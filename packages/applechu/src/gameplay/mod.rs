mod hotkeys;
mod restart_song;

use crate::config::Config;
use crate::util::api::Api;

crate::config_section! {
    pub(crate) struct RestartSongConfig => RESTART_SONG_CONFIG_SECTION {
        section: "RestartSong",
        order: 341,
        default_on: false,
        always_enabled: false,
        hidden: false,
        group: "gameplay",
        comment: "重新开始当前歌曲",
        fields: {
            pub hotkey: crate::config::VirtualKey = crate::config::VirtualKey::new(0x24),
            description: "重新开始当前歌曲",
            description_en: "Restart the current song",
            comment: "重开歌曲按键";
        }
    }
}

#[applechu_macros::config_section(stage = Late, order = 50, shutdown = shutdown)]
pub fn init(api: &Api, config: &Config) -> Result<(), String> {
    let Some(section) = config.section::<RestartSongConfig>() else {
        return Ok(());
    };
    if !section.enabled {
        return Ok(());
    }
    restart_song::init(api, section.hotkey.code())
}

pub fn shutdown() {
    restart_song::shutdown();
}
