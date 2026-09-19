//! The in-app settings menu.
//!
//! Opened with [`SETTINGS_KEY`]. It owns what can be changed without a restart:
//! whether the app is running in VR or on the flat screen, the frame-time
//! overlay, the present mode (v-sync), and the window mode.
//!
//! The menu is driven from the desktop window and stays available while a
//! headset is running, because switching *out* of VR has to be reachable from
//! somewhere. It is not visible inside the headset: Bevy's `Node` UI draws to
//! the window camera and never reaches the XR eye cameras.

use std::time::Duration;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig};
use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, PresentMode, PrimaryMonitor, PrimaryWindow, VideoModeSelection,
    WindowMode,
};
use bevy_mod_xr::session::{XrCreateSessionMessage, XrRequestExitMessage, XrState};

/// Opens and closes the settings menu.
pub const SETTINGS_KEY: KeyCode = KeyCode::Escape;

const ROW_IDLE: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);
const ROW_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const ROW_PRESSED: Color = Color::srgba(0.35, 0.75, 1.0, 0.30);

pub struct SettingsMenuPlugin;

impl Plugin for SettingsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FpsOverlayPlugin {
            config: FpsOverlayConfig {
                text_config: TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                text_color: Color::srgb(0.55, 1.0, 0.6),
                // Hidden until switched on from the menu. Bevy applies this on
                // the first frame, since its toggle runs on `resource_changed`
                // and an inserted resource counts as changed.
                enabled: false,
                refresh_interval: Duration::from_millis(100),
                frame_time_graph_config: FrameTimeGraphConfig {
                    enabled: false,
                    // Headset cadence, not monitor cadence: the point of
                    // watching frame times in a VR project is knowing whether
                    // you would hold 72-90 Hz, even while working flat.
                    min_fps: 72.0,
                    target_fps: 90.0,
                },
            },
        })
        .init_resource::<SettingsMenu>()
            .init_resource::<VideoSettings>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (
                    toggle_menu,
                    cycle_on_click,
                    highlight_rows,
                    apply_video_settings,
                    refresh_labels,
                )
                    .chain(),
            );
    }
}

/// Whether the menu is currently on screen.
#[derive(Resource, Default, Debug)]
pub struct SettingsMenu {
    pub open: bool,
}

/// Run condition for anything that should yield while the menu has focus.
pub fn settings_menu_open(menu: Option<Res<SettingsMenu>>) -> bool {
    menu.is_some_and(|menu| menu.open)
}

/// The window settings the menu edits. Defaults match the window Bevy creates,
/// so the first apply is a no-op rather than a visible jump at startup.
#[derive(Resource, Clone, Copy, Debug)]
pub struct VideoSettings {
    pub present_mode: PresentMode,
    pub screen_mode: ScreenMode,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            present_mode: PresentMode::AutoVsync,
            screen_mode: ScreenMode::Windowed,
        }
    }
}

/// The window modes worth offering, flattened out of [`WindowMode`] so they can
/// be cycled and labelled without carrying monitor and video-mode payloads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScreenMode {
    Windowed,
    BorderlessFullscreen,
    ExclusiveFullscreen,
}

impl ScreenMode {
    const ALL: [Self; 3] = [
        Self::Windowed,
        Self::BorderlessFullscreen,
        Self::ExclusiveFullscreen,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Windowed => "Windowed",
            Self::BorderlessFullscreen => "Borderless fullscreen",
            Self::ExclusiveFullscreen => "Exclusive fullscreen",
        }
    }

    /// `monitor` is a concrete [`Monitor`] entity to go fullscreen on, if one
    /// is known.
    ///
    /// Exclusive fullscreen needs it. Bevy resolves the monitor selection and
    /// **panics** if it comes back empty (`bevy_winit`'s `system.rs`:
    /// `"Could not find monitor for {selection}"`), and `MonitorSelection::Current`
    /// resolves to nothing on Wayland, where winit reports no current monitor
    /// for a window. Selecting the entity directly avoids that, and if no
    /// monitor is known at all this degrades to borderless rather than killing
    /// the app from a settings menu click.
    fn window_mode(self, monitor: Option<Entity>) -> WindowMode {
        match self {
            Self::Windowed => WindowMode::Windowed,
            Self::BorderlessFullscreen => {
                // Borderless tolerates an unresolved selection; only the
                // exclusive path panics.
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            }
            Self::ExclusiveFullscreen => match monitor {
                // `VideoModeSelection::Current` keeps the desktop's resolution
                // and refresh rate rather than switching the display.
                Some(monitor) => WindowMode::Fullscreen(
                    MonitorSelection::Entity(monitor),
                    VideoModeSelection::Current,
                ),
                None => {
                    warn!("no monitor to go exclusive-fullscreen on; using borderless");
                    WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                }
            },
        }
    }

    fn next(self) -> Self {
        next_in(&Self::ALL, |mode| *mode == self)
    }
}

/// Every present mode Bevy exposes.
///
/// All of them are safe to select: `bevy_render`'s `present_mode` picks the
/// closest supported option and always ends at `Fifo`, logging when it has to
/// substitute. So on a driver without, say, Mailbox, that row still works — it
/// just quietly gets something else.
const PRESENT_MODES: [(PresentMode, &str); 6] = [
    (PresentMode::AutoVsync, "On (auto)"),
    (PresentMode::AutoNoVsync, "Off (auto)"),
    (PresentMode::Fifo, "On - Fifo"),
    (PresentMode::FifoRelaxed, "Adaptive - Fifo relaxed"),
    (PresentMode::Mailbox, "Off - Mailbox"),
    (PresentMode::Immediate, "Off - Immediate"),
];

/// What the Mode row reads, given the current session state.
fn mode_label(state: Option<&XrState>) -> &'static str {
    match state {
        None | Some(XrState::Unavailable) => "Desktop (no VR runtime)",
        Some(XrState::Available) => "Desktop",
        Some(XrState::Running) => "VR",
        // Idle, Ready, Stopping, Exiting: a session exists but is not, or is no
        // longer, presenting.
        Some(_) => "VR (switching)",
    }
}

fn present_mode_label(mode: PresentMode) -> &'static str {
    PRESENT_MODES
        .iter()
        .find(|(candidate, _)| *candidate == mode)
        .map(|(_, label)| *label)
        .unwrap_or("Unknown")
}

fn next_present_mode(mode: PresentMode) -> PresentMode {
    next_in(&PRESENT_MODES, |(candidate, _)| *candidate == mode).0
}

/// The entry after the one `is_current` matches, wrapping at the end.
fn next_in<T: Copy>(options: &[T], is_current: impl Fn(&T) -> bool) -> T {
    let at = options.iter().position(is_current).unwrap_or(0);
    options[(at + 1) % options.len()]
}

#[derive(Component)]
struct SettingsPanel;

/// Which setting a clickable row edits.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum SettingRow {
    Mode,
    FrameStats,
    VSync,
    Display,
}

impl SettingRow {
    fn name(self) -> &'static str {
        match self {
            Self::Mode => "Mode",
            Self::FrameStats => "Frame stats",
            Self::VSync => "V-Sync",
            Self::Display => "Display",
        }
    }
}

/// Marks the text inside a row, so labels can be refreshed without walking
/// the hierarchy.
#[derive(Component, Clone, Copy)]
struct RowLabel(SettingRow);

fn spawn_menu(mut commands: Commands) {
    commands.spawn((
        Name::new("Settings Menu"),
        SettingsPanel,
        // Full-screen container, so the panel can centre itself instead of
        // competing with the FPS overlay for the top-left corner.
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            // Hidden until the key is pressed.
            display: Display::None,
            ..default()
        },
        // Above the help overlay and the FPS readout.
        GlobalZIndex(50),
        children![(
        Node {
            width: Val::Px(430.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(14.0)),
            row_gap: Val::Px(8.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.93)),
        children![
            (
                Text::new("Settings"),
                TextFont {
                    font_size: FontSize::Px(19.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ),
            setting_row(SettingRow::Mode),
            setting_row(SettingRow::FrameStats),
            setting_row(SettingRow::VSync),
            setting_row(SettingRow::Display),
            (
                Text::new("Click a row to cycle it - Esc to close"),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.58, 0.62, 0.7)),
            ),
        ],
        )],
    ));
}

fn setting_row(row: SettingRow) -> impl Bundle {
    (
        Button,
        row,
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            ..default()
        },
        BackgroundColor(ROW_IDLE),
        children![(
            Text::new(""),
            RowLabel(row),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.9, 0.95)),
        )],
    )
}

fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<SettingsMenu>,
    mut panel: Query<&mut Node, With<SettingsPanel>>,
) {
    if !keys.just_pressed(SETTINGS_KEY) {
        return;
    }
    menu.open = !menu.open;

    for mut node in &mut panel {
        node.display = if menu.open {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn cycle_on_click(
    rows: Query<(&Interaction, &SettingRow), Changed<Interaction>>,
    xr: Option<Res<XrState>>,
    mut settings: ResMut<VideoSettings>,
    mut overlay: ResMut<FpsOverlayConfig>,
    mut enter_vr: MessageWriter<XrCreateSessionMessage>,
    mut leave_vr: MessageWriter<XrRequestExitMessage>,
) {
    for (interaction, row) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match row {
            // Creating a session needs the runtime idle and waiting; leaving
            // one is only a request, and the backend walks the session through
            // Stopping, Exiting and Destroyed on its own.
            SettingRow::Mode => match xr.as_deref() {
                None | Some(XrState::Unavailable) => {
                    info!("no OpenXR runtime available, staying on the flat screen");
                }
                Some(XrState::Available) => {
                    enter_vr.write_default();
                }
                Some(_) => {
                    leave_vr.write_default();
                }
            },
            // Both flags have to move: Bevy drives the graph's visibility from
            // `frame_time_graph_config.enabled` alone, so flipping only the
            // outer one would hide the number and leave the graph on screen.
            SettingRow::FrameStats => {
                let showing = !overlay.enabled;
                overlay.enabled = showing;
                overlay.frame_time_graph_config.enabled = showing;
            }
            SettingRow::VSync => {
                settings.present_mode = next_present_mode(settings.present_mode);
            }
            SettingRow::Display => {
                settings.screen_mode = settings.screen_mode.next();
            }
        }
    }
}

fn highlight_rows(
    mut rows: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<SettingRow>)>,
) {
    for (interaction, mut background) in &mut rows {
        background.0 = match interaction {
            Interaction::Pressed => ROW_PRESSED,
            Interaction::Hovered => ROW_HOVER,
            Interaction::None => ROW_IDLE,
        };
    }
}

fn apply_video_settings(
    settings: Res<VideoSettings>,
    primary_monitor: Query<Entity, (With<Monitor>, With<PrimaryMonitor>)>,
    any_monitor: Query<Entity, With<Monitor>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !settings.is_changed() {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let monitor = primary_monitor.iter().chain(any_monitor.iter()).next();

    window.present_mode = settings.present_mode;
    window.mode = settings.screen_mode.window_mode(monitor);
}

fn refresh_labels(
    settings: Res<VideoSettings>,
    overlay: Res<FpsOverlayConfig>,
    xr: Option<Res<XrState>>,
    mut labels: Query<(&RowLabel, &mut Text)>,
) {
    let xr_changed = xr.as_ref().is_some_and(|state| state.is_changed());
    if !settings.is_changed() && !overlay.is_changed() && !xr_changed {
        return;
    }
    for (label, mut text) in &mut labels {
        let value = match label.0 {
            SettingRow::Mode => mode_label(xr.as_deref()),
            SettingRow::FrameStats => {
                if overlay.enabled {
                    "Shown"
                } else {
                    "Hidden"
                }
            }
            SettingRow::VSync => present_mode_label(settings.present_mode),
            SettingRow::Display => settings.screen_mode.label(),
        };
        text.0 = format!("{:<12} {value}", label.0.name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world with just the settings resources and the click handler — no
    /// renderer, no window.
    fn test_app() -> App {
        let mut app = App::new();
        app.init_resource::<VideoSettings>()
            .init_resource::<SettingsMenu>()
            // cycle_on_click writes these; without registration its
            // MessageWriter params fail validation and the system panics.
            .add_message::<XrCreateSessionMessage>()
            .add_message::<XrRequestExitMessage>()
            .insert_resource(FpsOverlayConfig {
                enabled: false,
                frame_time_graph_config: FrameTimeGraphConfig {
                    enabled: false,
                    ..default()
                },
                ..default()
            })
            .add_systems(Update, cycle_on_click);
        app
    }

    /// Spawning a row already `Pressed` makes `Changed<Interaction>` fire on
    /// the next run, which is what a real click looks like to the system.
    fn click(app: &mut App, row: SettingRow) {
        let entity = app.world_mut().spawn((Interaction::Pressed, row)).id();
        app.update();
        app.world_mut().entity_mut(entity).despawn();
    }

    #[test]
    fn frame_stats_row_moves_readout_and_graph_together() {
        let mut app = test_app();
        click(&mut app, SettingRow::FrameStats);

        let overlay = app.world().resource::<FpsOverlayConfig>();
        assert!(overlay.enabled, "FPS readout should be visible");
        assert!(
            overlay.frame_time_graph_config.enabled,
            "frame-time graph should be visible"
        );

        click(&mut app, SettingRow::FrameStats);
        let overlay = app.world().resource::<FpsOverlayConfig>();
        assert!(!overlay.enabled);
        assert!(!overlay.frame_time_graph_config.enabled);
    }

    #[test]
    fn vsync_row_advances_the_present_mode() {
        let mut app = test_app();
        assert_eq!(
            app.world().resource::<VideoSettings>().present_mode,
            PresentMode::AutoVsync
        );

        click(&mut app, SettingRow::VSync);
        assert_eq!(
            app.world().resource::<VideoSettings>().present_mode,
            PresentMode::AutoNoVsync
        );
    }

    #[test]
    fn display_row_advances_the_screen_mode() {
        let mut app = test_app();
        click(&mut app, SettingRow::Display);
        assert_eq!(
            app.world().resource::<VideoSettings>().screen_mode,
            ScreenMode::BorderlessFullscreen
        );

        click(&mut app, SettingRow::Display);
        assert_eq!(
            app.world().resource::<VideoSettings>().screen_mode,
            ScreenMode::ExclusiveFullscreen
        );
    }

    #[test]
    fn hovering_a_row_changes_nothing() {
        let mut app = test_app();
        app.world_mut()
            .spawn((Interaction::Hovered, SettingRow::VSync));
        app.update();

        assert_eq!(
            app.world().resource::<VideoSettings>().present_mode,
            PresentMode::AutoVsync
        );
    }

    #[test]
    fn every_present_mode_is_reachable_and_wraps() {
        let mut mode = PresentMode::AutoVsync;
        let mut seen = vec![mode];
        for _ in 1..PRESENT_MODES.len() {
            mode = next_present_mode(mode);
            assert!(!seen.contains(&mode), "{mode:?} repeated before wrapping");
            seen.push(mode);
        }
        assert_eq!(seen.len(), PRESENT_MODES.len());
        assert_eq!(next_present_mode(mode), PresentMode::AutoVsync, "should wrap");
    }

    #[test]
    fn every_screen_mode_is_reachable_and_wraps() {
        let mut mode = ScreenMode::Windowed;
        for _ in 0..ScreenMode::ALL.len() {
            mode = mode.next();
        }
        assert_eq!(mode, ScreenMode::Windowed, "a full lap returns to the start");
    }

    #[test]
    fn screen_modes_map_onto_bevy_window_modes() {
        let monitor = Some(Entity::from_raw_u32(1).unwrap());
        assert!(matches!(
            ScreenMode::Windowed.window_mode(monitor),
            WindowMode::Windowed
        ));
        assert!(matches!(
            ScreenMode::BorderlessFullscreen.window_mode(monitor),
            WindowMode::BorderlessFullscreen(_)
        ));
        assert!(matches!(
            ScreenMode::ExclusiveFullscreen.window_mode(monitor),
            WindowMode::Fullscreen(MonitorSelection::Entity(_), _)
        ));
    }

    /// Regression: exclusive fullscreen used to pass `MonitorSelection::Current`,
    /// which resolves to nothing on Wayland, and Bevy panics rather than
    /// degrading — so cycling the Display row killed the app.
    #[test]
    fn exclusive_fullscreen_without_a_monitor_degrades_instead_of_panicking() {
        assert!(matches!(
            ScreenMode::ExclusiveFullscreen.window_mode(None),
            WindowMode::BorderlessFullscreen(_)
        ));
    }

    #[test]
    fn every_present_mode_has_a_label() {
        for (mode, label) in PRESENT_MODES {
            assert_eq!(present_mode_label(mode), label);
        }
    }

    /// How many messages of this type were written, draining them.
    fn written<M: Message>(app: &mut App) -> usize {
        app.world_mut().resource_mut::<Messages<M>>().drain().count()
    }

    fn app_in_state(state: XrState) -> App {
        let mut app = test_app();
        app.insert_resource(state);
        app
    }

    #[test]
    fn mode_row_enters_vr_when_a_session_is_available() {
        let mut app = app_in_state(XrState::Available);
        click(&mut app, SettingRow::Mode);

        assert_eq!(written::<XrCreateSessionMessage>(&mut app), 1);
        assert_eq!(written::<XrRequestExitMessage>(&mut app), 0);
    }

    #[test]
    fn mode_row_leaves_vr_while_a_session_runs() {
        let mut app = app_in_state(XrState::Running);
        click(&mut app, SettingRow::Mode);

        assert_eq!(written::<XrRequestExitMessage>(&mut app), 1);
        assert_eq!(written::<XrCreateSessionMessage>(&mut app), 0);
    }

    #[test]
    fn mode_row_does_nothing_without_a_runtime() {
        let mut app = app_in_state(XrState::Unavailable);
        click(&mut app, SettingRow::Mode);

        assert_eq!(written::<XrCreateSessionMessage>(&mut app), 0);
        assert_eq!(written::<XrRequestExitMessage>(&mut app), 0);
    }

    #[test]
    fn mode_row_reads_the_session_state() {
        assert_eq!(mode_label(None), "Desktop (no VR runtime)");
        assert_eq!(
            mode_label(Some(&XrState::Unavailable)),
            "Desktop (no VR runtime)"
        );
        assert_eq!(mode_label(Some(&XrState::Available)), "Desktop");
        assert_eq!(mode_label(Some(&XrState::Running)), "VR");
        assert_eq!(mode_label(Some(&XrState::Ready)), "VR (switching)");
    }

    #[test]
    fn the_menu_is_closed_until_opened() {
        let app = test_app();
        assert!(!app.world().resource::<SettingsMenu>().open);
    }
}
