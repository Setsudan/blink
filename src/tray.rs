use eframe::egui;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    BreakNow,
    TogglePause,
    ToggleSound,
    Quit,
}

pub struct AppTray {
    tray: TrayIcon,
    break_now_id: MenuId,
    pause_id: MenuId,
    sound_id: MenuId,
    quit_id: MenuId,
    pause_item: MenuItem,
    sound_item: CheckMenuItem,
    paused: bool,
    last_tooltip: String,
}

impl AppTray {
    pub fn new(sound_enabled: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let icon = load_tray_icon()?;
        let menu = Menu::new();
        let break_now = MenuItem::new("Break now", true, None);
        let pause = MenuItem::new("Pause", true, None);
        let sound = CheckMenuItem::new("Sound", true, sound_enabled, None);
        let quit = MenuItem::new("Quit Blink", true, None);
        menu.append(&break_now)?;
        menu.append(&pause)?;
        menu.append(&sound)?;
        menu.append(&quit)?;

        let tooltip = "Blink  eye break reminder".to_owned();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(&tooltip)
            .with_icon(icon)
            .build()?;

        Ok(Self {
            tray,
            break_now_id: break_now.id().clone(),
            pause_id: pause.id().clone(),
            sound_id: sound.id().clone(),
            quit_id: quit.id().clone(),
            pause_item: pause,
            sound_item: sound,
            paused: false,
            last_tooltip: tooltip,
        })
    }

    pub fn poll(&self) -> Vec<TrayAction> {
        MenuEvent::receiver()
            .try_iter()
            .filter_map(|event| {
                if event.id == self.quit_id {
                    Some(TrayAction::Quit)
                } else if event.id == self.break_now_id {
                    Some(TrayAction::BreakNow)
                } else if event.id == self.pause_id {
                    Some(TrayAction::TogglePause)
                } else if event.id == self.sound_id {
                    Some(TrayAction::ToggleSound)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        let label = if paused { "Resume" } else { "Pause" };
        self.pause_item.set_text(label);
    }

    pub fn set_sound_enabled(&self, enabled: bool) {
        self.sound_item.set_checked(enabled);
    }

    pub fn set_tooltip(&mut self, text: &str) {
        if self.last_tooltip == text {
            return;
        }
        if self.tray.set_tooltip(Some(text)).is_ok() {
            self.last_tooltip = text.to_owned();
        }
    }
}

fn load_tray_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    let svg = include_str!("../assets/eye.svg").replace("currentColor", "#3a7fc4");
    let image = egui_extras::image::load_svg_bytes_with_size(
        svg.as_bytes(),
        egui::load::SizeHint::Size {
            width: 32,
            height: 32,
            maintain_aspect_ratio: true,
        },
        &Default::default(),
    )?;

    let [width, height] = image.size;
    let rgba: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| [c.r(), c.g(), c.b(), c.a()])
        .collect();

    Ok(Icon::from_rgba(rgba, width as u32, height as u32)?)
}
