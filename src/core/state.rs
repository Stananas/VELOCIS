#![allow(dead_code)]

use std::path::PathBuf;
use std::time::SystemTime;

gpui::actions!(state_mod, [CommandPalette, GoHome, NewProject, SaveProject]);

#[derive(Clone)]
pub struct ProjectMeta {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub last_opened: SystemTime,
}

#[derive(Clone, PartialEq)]
pub enum AppView {
    Home,
    Editor,
}

#[derive(Clone)]
pub struct Settings {
    pub theme: String,
    pub autosave: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            autosave: true,
        }
    }
}
