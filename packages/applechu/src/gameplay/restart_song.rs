use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::gameplay::hotkeys::EdgeHotkey;
use crate::util::api::Api;

const CURRENT_STATE_OFFSET: usize = 0x10;
const PENDING_STATE_OFFSET: usize = 0x14;
const LOAD_STATE: i32 = 0;
const PLAY_STATE: i32 = 15;
const PARTY_WAIT_STATE: i32 = 16;
const PLAY_UPDATE_PATTERN: &[u8] = &[
    0x53, 0x56, 0x57, 0x6A, 0x01, 0x8B, 0xF1, 0xE8, 0, 0, 0, 0, 0x6A, 0x00, 0x8B, 0xCE, 0xE8, 0, 0,
    0, 0, 0x8B, 0x1D, 0, 0, 0, 0, 0x8B, 0xCB,
];
const PLAY_UPDATE_MASK: &str = "xxxxxxxx????xxxxx????xx????xx";
const PARTY_WAIT_UPDATE_PATTERN: &[u8] = &[
    0x83, 0xEC, 0x08, 0x53, 0x56, 0x57, 0x6A, 0x01, 0x8B, 0xF1, 0xE8, 0, 0, 0, 0, 0x6A, 0x01, 0x8B,
    0xCE,
];
const PARTY_WAIT_UPDATE_MASK: &str = "xxxxxxxxxxx????xxxx";
const KNOWN_PLAY_UPDATE_RVAS: &[usize] = &[0x9D42D0];
const KNOWN_PARTY_WAIT_UPDATE_RVAS: &[usize] = &[0x9D3BD0];

type UpdateFn = unsafe extern "thiscall" fn(*mut u8) -> *mut c_void;

static API: OnceLock<Api> = OnceLock::new();
static PLAY_TARGET: AtomicUsize = AtomicUsize::new(0);
static PLAY_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static PARTY_WAIT_TARGET: AtomicUsize = AtomicUsize::new(0);
static PARTY_WAIT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOTKEY: EdgeHotkey = EdgeHotkey::new();
static RESTART_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn init(api: &Api, key: i32) -> Result<(), String> {
    let _ = API.set(*api);
    install_play_hook(api)?;
    install_party_wait_hook(api)?;
    HOTKEY.configure(key);
    Ok(())
}

pub fn shutdown() {
    HOTKEY.reset();
    RESTART_ACTIVE.store(false, Ordering::Release);
    if let Some(api) = API.get() {
        remove_hook(api, &PLAY_TARGET, &PLAY_ORIGINAL);
        remove_hook(api, &PARTY_WAIT_TARGET, &PARTY_WAIT_ORIGINAL);
    }
}

fn install_play_hook(api: &Api) -> Result<(), String> {
    let target = find_update(
        api,
        PLAY_UPDATE_PATTERN,
        PLAY_UPDATE_MASK,
        KNOWN_PLAY_UPDATE_RVAS,
    )
    .ok_or_else(|| "PlayMusic update method was not found".to_owned())?;
    install_hook(
        api,
        target,
        hooked_play_update as *const () as usize,
        &PLAY_TARGET,
        &PLAY_ORIGINAL,
        "PlayMusic update",
    )
}

fn install_party_wait_hook(api: &Api) -> Result<(), String> {
    let target = find_update(
        api,
        PARTY_WAIT_UPDATE_PATTERN,
        PARTY_WAIT_UPDATE_MASK,
        KNOWN_PARTY_WAIT_UPDATE_RVAS,
    )
    .ok_or_else(|| "PartyWait update method was not found".to_owned())?;
    install_hook(
        api,
        target,
        hooked_party_wait_update as *const () as usize,
        &PARTY_WAIT_TARGET,
        &PARTY_WAIT_ORIGINAL,
        "PartyWait update",
    )
}

fn install_hook(
    api: &Api,
    target: usize,
    detour: usize,
    target_slot: &AtomicUsize,
    original_slot: &AtomicUsize,
    name: &str,
) -> Result<(), String> {
    let Some(trampoline) = api.hook_create(target, detour) else {
        return Err(format!("{name} hook could not be created"));
    };
    original_slot.store(trampoline, Ordering::Release);
    target_slot.store(target, Ordering::Release);
    if !api.hook_enable(target) {
        api.hook_remove(target);
        target_slot.store(0, Ordering::Release);
        original_slot.store(0, Ordering::Release);
        return Err(format!("{name} hook could not be enabled"));
    }
    api.log_info(&format!(
        "RestartSong hook enabled: {name} @ 0x{target:08X}"
    ));
    Ok(())
}

fn remove_hook(api: &Api, target_slot: &AtomicUsize, original_slot: &AtomicUsize) {
    let target = target_slot.swap(0, Ordering::AcqRel);
    if target != 0 {
        api.hook_disable(target);
        api.hook_remove(target);
    }
    original_slot.store(0, Ordering::Release);
}

fn find_update(api: &Api, pattern: &[u8], mask: &str, known_rvas: &[usize]) -> Option<usize> {
    let text = api.aob_scan(api.text_base(), api.text_size(), pattern, mask);
    if text != 0 {
        return Some(text);
    }
    let image = api.aob_scan(api.game_base(), api.game_size(), pattern, mask);
    if image != 0 {
        return Some(image);
    }

    let text_start = api.text_base();
    let text_end = text_start.checked_add(api.text_size() as usize)?;
    for rva in known_rvas {
        let target = api.game_base().checked_add(*rva)?;
        if (text_start..text_end).contains(&target) && matches_pattern(api, target, pattern, mask) {
            return Some(target);
        }
    }
    None
}

fn matches_pattern(api: &Api, target: usize, pattern: &[u8], mask: &str) -> bool {
    if pattern.len() != mask.len() {
        return false;
    }
    let mut bytes = vec![0; pattern.len()];
    api.mem_read(target, &mut bytes)
        && bytes
            .iter()
            .zip(pattern.iter().zip(mask.as_bytes()))
            .all(|(actual, (expected, kind))| *kind != b'x' || actual == expected)
}

unsafe extern "fastcall" fn hooked_play_update(this: *mut u8, _edx: *mut c_void) -> *mut c_void {
    let original_address = PLAY_ORIGINAL.load(Ordering::Acquire);
    if original_address == 0 || this.is_null() {
        return std::ptr::null_mut();
    }
    let original: UpdateFn = std::mem::transmute(original_address);
    let current = this
        .add(CURRENT_STATE_OFFSET)
        .cast::<i32>()
        .read_unaligned();
    if current != PLAY_STATE {
        HOTKEY.reset();
        RESTART_ACTIVE.store(false, Ordering::Release);
    }

    let result = original(this);
    let current = this
        .add(CURRENT_STATE_OFFSET)
        .cast::<i32>()
        .read_unaligned();
    let pending = this
        .add(PENDING_STATE_OFFSET)
        .cast::<i32>()
        .read_unaligned();
    if current == PLAY_STATE && pending < 0 && HOTKEY.poll() {
        RESTART_ACTIVE.store(true, Ordering::Release);
        this.add(PENDING_STATE_OFFSET)
            .cast::<i32>()
            .write_unaligned(PARTY_WAIT_STATE);
        if let Some(api) = API.get() {
            api.log_info("RestartSong requested");
        }
    }
    result
}

unsafe extern "fastcall" fn hooked_party_wait_update(
    this: *mut u8,
    _edx: *mut c_void,
) -> *mut c_void {
    let original_address = PARTY_WAIT_ORIGINAL.load(Ordering::Acquire);
    if original_address == 0 || this.is_null() {
        return std::ptr::null_mut();
    }
    let current = this
        .add(CURRENT_STATE_OFFSET)
        .cast::<i32>()
        .read_unaligned();
    if current == PARTY_WAIT_STATE && RESTART_ACTIVE.swap(false, Ordering::AcqRel) {
        // PartyWait 初始化已经执行原生停歌，这里跳过结算分支并重新加载歌曲
        this.add(PENDING_STATE_OFFSET)
            .cast::<i32>()
            .write_unaligned(LOAD_STATE);
        if let Some(api) = API.get() {
            api.log_info("RestartSong entering Load state");
        }
        return std::ptr::null_mut();
    }
    if current != PARTY_WAIT_STATE {
        RESTART_ACTIVE.store(false, Ordering::Release);
    }
    let original: UpdateFn = std::mem::transmute(original_address);
    original(this)
}
