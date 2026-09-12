#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorChatKey {
    pub actor_user_id: u64,
    pub chat_id: i64,
}

impl ActorChatKey {
    pub const fn new(actor_user_id: u64, chat_id: i64) -> Self {
        Self {
            actor_user_id,
            chat_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestIdentity {
    pub actor_user_id: u64,
    pub chat_id: i64,
    pub message_id: i32,
    pub is_private: bool,
}

impl RequestIdentity {
    pub const fn actor_chat_key(self) -> ActorChatKey {
        ActorChatKey::new(self.actor_user_id, self.chat_id)
    }

    pub const fn is_private_admin(self, admin_user_id: u64, admin_chat_id: i64) -> bool {
        self.is_private && self.actor_user_id == admin_user_id && self.chat_id == admin_chat_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInputAction {
    AddWallet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveAction {
    Pause,
    Resume,
    CleanupEvents,
    MuteAlerts,
    UnmuteAlerts,
    ClearWallets,
    ForgetAll,
    ToggleMemoryCleaner,
    ToggleLiveSync,
    ToggleMaintenance,
}

impl SensitiveAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::CleanupEvents => "cleanup_events",
            Self::MuteAlerts => "mute_alerts",
            Self::UnmuteAlerts => "unmute_alerts",
            Self::ClearWallets => "clear_wallets",
            Self::ForgetAll => "forget_all",
            Self::ToggleMemoryCleaner => "toggle_memory",
            Self::ToggleLiveSync => "toggle_live_sync",
            Self::ToggleMaintenance => "toggle_maintenance",
        }
    }

    pub const fn execute_callback(self) -> &'static str {
        match self {
            Self::Pause => "do_pause",
            Self::Resume => "do_resume",
            Self::CleanupEvents => "do_cleanup_events",
            Self::MuteAlerts => "do_mute_alerts",
            Self::UnmuteAlerts => "do_unmute_alerts",
            Self::ClearWallets => "do_forget_wallets",
            Self::ForgetAll => "do_forget_all",
            Self::ToggleMemoryCleaner => "btn_toggle_ENABLE_MEMORY_CLEANER",
            Self::ToggleLiveSync => "btn_toggle_ENABLE_LIVE_SYNC",
            Self::ToggleMaintenance => "btn_toggle_MAINTENANCE_MODE",
        }
    }

    pub const fn requires_admin(self) -> bool {
        !matches!(self, Self::ClearWallets | Self::ForgetAll)
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "cleanup_events" => Some(Self::CleanupEvents),
            "mute_alerts" => Some(Self::MuteAlerts),
            "unmute_alerts" => Some(Self::UnmuteAlerts),
            "clear_wallets" => Some(Self::ClearWallets),
            "forget_all" => Some(Self::ForgetAll),
            "toggle_memory" => Some(Self::ToggleMemoryCleaner),
            "toggle_live_sync" => Some(Self::ToggleLiveSync),
            "toggle_maintenance" => Some(Self::ToggleMaintenance),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationSession {
    pub actor_user_id: u64,
    pub chat_id: i64,
    pub message_id: i32,
    pub action: SensitiveAction,
    pub expires_at_unix_secs: u64,
}
