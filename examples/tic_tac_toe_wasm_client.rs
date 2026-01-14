//! Tic-tac-toe WASM client example.
//! Can act as both host (server) and client, connecting via a separate signaling server.
//! Designed for WebAssembly deployment.
//!
//! Usage:
//! - Host: ?host=true&lobby=CODE (or just ?host=true)
//! - Client: ?lobby=CODE (or no parameters for default lobby)

use std::fmt::{self, Formatter};

use bevy::{
    ecs::{relationship::RelatedSpawner, spawn::SpawnWith},
    prelude::*,
};
use bevy_replicon::prelude::*;
use bevy_replicon_matchbox::{MatchboxClient, MatchboxHost, RepliconMatchboxPlugins};
use serde::{Deserialize, Serialize};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.build().set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Tic-Tac-Toe (WASM Client)".into(),
                    resolution: (800, 600).into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            RepliconPlugins,
            RepliconMatchboxPlugins,
        ))
        .init_state::<GameState>()
        .init_resource::<SymbolFont>()
        .init_resource::<TurnSymbol>()
        .replicate::<Symbol>()
        .add_client_event::<PickCell>(Channel::Ordered)
        .insert_resource(ClearColor(BACKGROUND_COLOR))
        .add_observer(init_symbols)
        .add_observer(advance_turn)
        .add_observer(init_client)
        .add_observer(apply_pick)
        .add_observer(disconnect_by_client)
        .add_systems(
            Startup,
            (
                (read_lobby_code_from_url, connect_to_server).chain(),
                setup_ui,
            ),
        )
        .add_systems(
            OnEnter(GameState::InGame),
            (show_turn_text, show_turn_symbol),
        )
        .add_systems(OnEnter(GameState::Disconnected), show_disconnected_text)
        .add_systems(OnEnter(GameState::Winner), show_winner_text)
        .add_systems(OnEnter(GameState::Tie), show_tie_text)
        .add_systems(OnEnter(GameState::Disconnected), stop_networking)
        .add_systems(OnEnter(ClientState::Connected), client_start)
        .add_systems(OnEnter(ClientState::Connecting), show_connecting_text)
        .add_systems(OnExit(ClientState::Connected), disconnect_by_server)
        .add_systems(OnEnter(ServerState::Running), show_waiting_client_text)
        .add_systems(
            Update,
            (
                update_buttons_background.run_if(local_player_turn),
                show_turn_symbol.run_if(resource_changed::<TurnSymbol>),
            )
                .run_if(in_state(GameState::InGame)),
        )
        .run();
}

const GRID_SIZE: usize = 3;

const BACKGROUND_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);

// Bottom text defined in two sections, first for text and second for symbols with different font.
const TEXT_SECTION: usize = 0;
const SYMBOL_SECTION: usize = 1;

const CELL_SIZE: f32 = 100.0;
const LINE_THICKNESS: f32 = 10.0;

const BUTTON_SIZE: f32 = CELL_SIZE / 1.2;
const BUTTON_MARGIN: f32 = (CELL_SIZE + LINE_THICKNESS - BUTTON_SIZE) / 2.0;

// Signaling server base URL - change this to your VPS URL when deploying
// For local development with signaling server on port 3536:
const SIGNALING_SERVER_BASE: &str = "ws://localhost:3536";
// For production, use: "wss://your-vps-domain.com:443"

// Default lobby code if none provided in URL
const DEFAULT_LOBBY_CODE: &str = "tic-tac-toe";

/// Stores the lobby code to connect to.
#[derive(Resource, Default)]
struct LobbyCode(String);

/// Stores whether this instance should act as the host (server).
#[derive(Resource, Default)]
struct IsHost(bool);

/// Reads lobby code and host flag from URL query parameters (for WASM) or uses defaults.
///
/// In WASM, reads from `?lobby=CODE&host=true` query parameters.
/// Falls back to DEFAULT_LOBBY_CODE if not found.
/// Host mode is enabled if `host=true` is in the URL.
fn read_lobby_code_from_url(mut commands: Commands) {
    let mut lobby_code = DEFAULT_LOBBY_CODE.to_string();
    let mut is_host = false;

    #[cfg(target_arch = "wasm32")]
    {
        // Try to read from URL query parameters
        use wasm_bindgen::JsCast;
        use web_sys::window;

        if let Some(window) = window() {
            if let Ok(search) = window.location().search() {
                // Parse query params: ?lobby=ABC123&host=true
                let params: Vec<&str> = search.trim_start_matches('?').split('&').collect();
                for param in params {
                    if let Some((key, value)) = param.split_once('=') {
                        match key {
                            "lobby" if !value.is_empty() => {
                                lobby_code = value.to_string();
                                info!("Found lobby code in URL: {}", lobby_code);
                            }
                            "host" if value == "true" => {
                                is_host = true;
                                info!("Host mode enabled via URL parameter");
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    commands.insert_resource(LobbyCode(lobby_code.clone()));
    commands.insert_resource(IsHost(is_host));
    info!("Using lobby code: {}, Host mode: {}", lobby_code, is_host);
}

fn connect_to_server(
    mut commands: Commands,
    replicon_channels: Res<RepliconChannels>,
    lobby_code: Res<LobbyCode>,
    is_host: Res<IsHost>,
) {
    let room_url = format!("{}/{}", SIGNALING_SERVER_BASE, lobby_code.0);

    if is_host.0 {
        // Host mode: create server and spawn as Cross player
        info!(
            "Starting as host (server) on signaling server: {}",
            room_url
        );
        match MatchboxHost::new(room_url, &replicon_channels) {
            Ok(host) => {
                commands.insert_resource(host);
                commands.spawn((LocalPlayer, Symbol::Cross));
                info!("Host started successfully");
            }
            Err(e) => {
                error!("Failed to create host: {:?}", e);
                commands.set_state(GameState::Disconnected);
            }
        }
    } else {
        // Client mode: connect to host
        info!("Connecting to signaling server: {}", room_url);
        match MatchboxClient::new(room_url, &replicon_channels) {
            Ok(client) => {
                commands.insert_resource(client);
                commands.spawn((LocalPlayer, ClientPlayer));
            }
            Err(e) => {
                error!("Failed to create client: {:?}", e);
                commands.set_state(GameState::Disconnected);
            }
        }
    }
}

fn setup_ui(mut commands: Commands, symbol_font: Res<SymbolFont>) {
    info!("setting up UI");
    commands.spawn(Camera2d);

    const LINES_COUNT: usize = GRID_SIZE + 1;
    const BOARD_SIZE: f32 = CELL_SIZE * GRID_SIZE as f32 + LINES_COUNT as f32 * LINE_THICKNESS;
    const BOARD_COLOR: Color = Color::srgb(0.8, 0.8, 0.8);

    for line in 0..LINES_COUNT {
        let position =
            -BOARD_SIZE / 2.0 + line as f32 * (CELL_SIZE + LINE_THICKNESS) + LINE_THICKNESS / 2.0;

        // Horizontal
        commands.spawn((
            Sprite {
                color: BOARD_COLOR,
                ..Default::default()
            },
            Transform {
                translation: Vec3::Y * position,
                scale: Vec3::new(BOARD_SIZE, LINE_THICKNESS, 1.0),
                ..Default::default()
            },
        ));

        // Vertical
        commands.spawn((
            Sprite {
                color: BOARD_COLOR,
                ..Default::default()
            },
            Transform {
                translation: Vec3::X * position,
                scale: Vec3::new(LINE_THICKNESS, BOARD_SIZE, 1.0),
                ..Default::default()
            },
        ));
    }

    const TEXT_COLOR: Color = Color::srgb(0.5, 0.5, 1.0);
    const FONT_SIZE: f32 = 32.0;

    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..Default::default()
        },
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(BOARD_SIZE - LINE_THICKNESS),
                height: Val::Px(BOARD_SIZE - LINE_THICKNESS),
                ..Default::default()
            },
            children![
                (
                    Node {
                        display: Display::Grid,
                        grid_template_columns: vec![GridTrack::auto(); GRID_SIZE],
                        ..Default::default()
                    },
                    Children::spawn(SpawnWith(|parent: &mut RelatedSpawner<_>| {
                        for index in 0..GRID_SIZE * GRID_SIZE {
                            parent.spawn(Cell { index }).observe(pick_cell);
                        }
                    }))
                ),
                (
                    Node {
                        margin: UiRect::top(Val::Px(20.0)),
                        justify_content: JustifyContent::Center,
                        ..Default::default()
                    },
                    children![(
                        Text::default(),
                        TextFont {
                            font_size: FONT_SIZE,
                            ..Default::default()
                        },
                        TextColor(TEXT_COLOR),
                        BottomText,
                        children![(
                            TextSpan::default(),
                            TextFont {
                                font: symbol_font.0.clone(),
                                font_size: FONT_SIZE,
                                ..Default::default()
                            },
                            TextColor(TEXT_COLOR),
                        )]
                    )]
                )
            ]
        )],
    ));
}

/// Converts point clicks into cell picking events.
///
/// For clients: sends PickCell event to server.
/// For host: directly applies the pick (server-authoritative).
fn pick_cell(
    click: On<Pointer<Click>>,
    mut commands: Commands,
    turn_symbol: Res<TurnSymbol>,
    game_state: Res<State<GameState>>,
    cells: Query<(Entity, &Cell), Without<Symbol>>,
    players: Query<&Symbol, With<LocalPlayer>>,
    is_host: Option<Res<IsHost>>,
) {
    if *game_state != GameState::InGame {
        return;
    }

    // Check if it's the local player's turn
    let current_turn = **turn_symbol;
    if !players.iter().any(|&symbol| symbol == current_turn) {
        return;
    }

    // Find the cell that was clicked
    let Ok((entity, cell)) = cells.get(click.entity) else {
        return;
    };

    let is_host_mode = is_host.map(|h| h.0).unwrap_or(false);

    if is_host_mode {
        // Host: directly apply the pick
        let player_symbol = players
            .iter()
            .find(|_| true)
            .copied()
            .expect("host should have a symbol");
        if player_symbol == current_turn {
            info!("Host picking cell {}", cell.index);
            commands.entity(entity).insert(current_turn);
        }
    } else {
        // Client: send event to server
        info!("Client picking cell {}", cell.index);
        commands.client_trigger(PickCell { index: cell.index });
    }
}

/// Initializes spawned symbol on client after replication.
fn init_symbols(
    add: On<Add, Symbol>,
    mut commands: Commands,
    symbol_font: Res<SymbolFont>,
    mut cells: Query<(&mut BackgroundColor, &Symbol), With<Button>>,
) {
    let Ok((mut background, symbol)) = cells.get_mut(add.entity) else {
        return;
    };
    *background = BACKGROUND_COLOR.into();

    commands
        .entity(add.entity)
        .remove::<Interaction>()
        .with_child((
            Text::new(symbol.glyph()),
            TextFont {
                font: symbol_font.0.clone(),
                font_size: 65.0,
                ..Default::default()
            },
            TextColor(symbol.color()),
        ));
}

/// Starts the game after connection (client mode only).
fn client_start(mut commands: Commands, lobby_code: Res<LobbyCode>) {
    let room_url = format!("{}/{}", SIGNALING_SERVER_BASE, lobby_code.0);
    info!("Successfully connected to signaling server: {}", room_url);
    commands.set_state(GameState::InGame);
}

/// Associates client with a symbol and starts the game (host mode only).
fn init_client(
    add: On<Add, AuthorizedClient>,
    mut commands: Commands,
    server_symbol: Single<&Symbol, With<LocalPlayer>>,
) {
    // Assign the opposite symbol to the client
    let client_symbol = server_symbol.next();

    // Utilize client entity as a player for convenient lookups by `client`.
    commands.entity(add.entity).insert((
        ClientPlayer,
        Signature::of::<ClientPlayer>(),
        client_symbol,
    ));

    info!("Client connected, assigned symbol: {}", client_symbol);
    commands.set_state(GameState::InGame);
}

/// Handles cell pick events from clients (host mode only).
fn apply_pick(
    pick: On<FromClient<PickCell>>,
    mut commands: Commands,
    cells: Query<(Entity, &Cell), Without<Symbol>>,
    turn_symbol: Res<TurnSymbol>,
    players: Query<&Symbol>,
) {
    // It's good to check the received data because client could be cheating.
    if let ClientId::Client(client) = pick.client_id {
        let symbol = *players
            .get(client)
            .expect("all clients should have assigned symbols");
        if symbol != **turn_symbol {
            error!("`{client}` chose cell {} at wrong turn", pick.index);
            return;
        }
    }

    let Some((entity, _)) = cells.iter().find(|(_, cell)| cell.index == pick.index) else {
        error!(
            "`{}` has chosen occupied or invalid cell {}",
            pick.client_id, pick.index
        );
        return;
    };

    info!(
        "Server applying pick: cell {} by {}",
        pick.index, pick.client_id
    );
    commands.entity(entity).insert(**turn_symbol);
}

/// Sets the game in disconnected state if client closes the connection (host mode only).
fn disconnect_by_client(
    _on: On<Remove, ConnectedClient>,
    game_state: Res<State<GameState>>,
    mut commands: Commands,
) {
    info!("client closed the connection");
    if *game_state == GameState::InGame {
        commands.set_state(GameState::Disconnected);
    }
}

/// Sets the game in disconnected state if server closes the connection.
fn disconnect_by_server(mut commands: Commands) {
    info!("server closed the connection");
    commands.set_state(GameState::Disconnected);
}

/// Closes all sockets.
fn stop_networking(mut commands: Commands) {
    commands.remove_resource::<MatchboxClient>();
    commands.remove_resource::<MatchboxHost>();
}

/// Checks the winner and advances the turn.
fn advance_turn(
    _on: On<Add, Symbol>,
    mut commands: Commands,
    mut turn_symbol: ResMut<TurnSymbol>,
    symbols: Query<(&Cell, &Symbol)>,
) {
    let mut board = [None; GRID_SIZE * GRID_SIZE];
    for (cell, &symbol) in &symbols {
        board[cell.index] = Some(symbol);
    }

    const WIN_CONDITIONS: [[usize; GRID_SIZE]; 8] = [
        [0, 1, 2],
        [3, 4, 5],
        [6, 7, 8],
        [0, 3, 6],
        [1, 4, 7],
        [2, 5, 8],
        [0, 4, 8],
        [2, 4, 6],
    ];

    for indices in WIN_CONDITIONS {
        let symbols = indices.map(|index| board[index]);
        if symbols[0].is_some() && symbols.windows(2).all(|symbols| symbols[0] == symbols[1]) {
            commands.set_state(GameState::Winner);
            info!("{} wins the game", **turn_symbol);
            return;
        }
    }

    if board.iter().all(Option::is_some) {
        info!("game ended in a tie");
        commands.set_state(GameState::Tie);
    } else {
        **turn_symbol = turn_symbol.next();
    }
}

fn update_buttons_background(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), Changed<Interaction>>,
) {
    const HOVER_COLOR: Color = Color::srgb(0.85, 0.85, 0.85);
    const PRESS_COLOR: Color = Color::srgb(0.95, 0.95, 0.95);

    for (interaction, mut background) in &mut buttons {
        match interaction {
            Interaction::Pressed => *background = PRESS_COLOR.into(),
            Interaction::Hovered => *background = HOVER_COLOR.into(),
            Interaction::None => *background = BACKGROUND_COLOR.into(),
        };
    }
}

fn show_turn_text(mut writer: TextUiWriter, text: Single<Entity, With<BottomText>>) {
    *writer.text(*text, TEXT_SECTION) = "Current turn: ".into();
}

fn show_turn_symbol(
    mut writer: TextUiWriter,
    turn_symbol: Res<TurnSymbol>,
    text: Single<Entity, With<BottomText>>,
) {
    *writer.text(*text, SYMBOL_SECTION) = turn_symbol.glyph().into();
    *writer.color(*text, SYMBOL_SECTION) = turn_symbol.color().into();
}

fn show_disconnected_text(mut writer: TextUiWriter, text: Single<Entity, With<BottomText>>) {
    *writer.text(*text, TEXT_SECTION) = "Disconnected".into();
    writer.text(*text, SYMBOL_SECTION).clear();
}

fn show_winner_text(mut writer: TextUiWriter, text: Single<Entity, With<BottomText>>) {
    *writer.text(*text, TEXT_SECTION) = "Winner: ".into();
}

fn show_tie_text(mut writer: TextUiWriter, text: Single<Entity, With<BottomText>>) {
    *writer.text(*text, TEXT_SECTION) = "Tie".into();
    writer.text(*text, SYMBOL_SECTION).clear();
}

fn show_connecting_text(mut writer: TextUiWriter, text: Single<Entity, With<BottomText>>) {
    *writer.text(*text, TEXT_SECTION) = "Connecting".into();
}

fn show_waiting_client_text(mut writer: TextUiWriter, text: Single<Entity, With<BottomText>>) {
    *writer.text(*text, TEXT_SECTION) = "Waiting client".into();
}

/// Returns `true` if the local player can select cells.
fn local_player_turn(
    turn_symbol: Res<TurnSymbol>,
    players: Query<&Symbol, With<LocalPlayer>>,
) -> bool {
    players.iter().any(|&symbol| symbol == **turn_symbol)
}

/// Font to display unicode characters for [`Symbol`].
#[derive(Resource)]
struct SymbolFont(Handle<Font>);

impl FromWorld for SymbolFont {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>();
        Self(asset_server.load("NotoEmoji-Regular.ttf"))
    }
}

#[derive(States, Clone, Copy, Debug, Eq, Hash, PartialEq, Default)]
enum GameState {
    #[default]
    WaitingPlayer,
    InGame,
    Winner,
    Tie,
    Disconnected,
}

/// Contains symbol to be used this turn.
#[derive(Resource, Default, Deref, DerefMut)]
struct TurnSymbol(Symbol);

/// The player's symbol, current [`TurnSymbol`] or a symbol of a filled cell.
#[derive(Component, Default, Serialize, Deserialize, Eq, PartialEq, Clone, Copy, Debug)]
enum Symbol {
    #[default]
    Cross,
    Nought,
}

impl Symbol {
    fn glyph(self) -> &'static str {
        match self {
            Symbol::Cross => "❌",
            Symbol::Nought => "⭕",
        }
    }

    fn color(self) -> Color {
        match self {
            Symbol::Cross => Color::srgb(1.0, 0.5, 0.5),
            Symbol::Nought => Color::srgb(0.5, 0.5, 1.0),
        }
    }

    fn next(self) -> Self {
        match self {
            Symbol::Cross => Symbol::Nought,
            Symbol::Nought => Symbol::Cross,
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Symbol::Cross => f.write_str("cross"),
            Symbol::Nought => f.write_str("nought"),
        }
    }
}

/// Marker for UI node with bottom text.
#[derive(Component)]
struct BottomText;

/// Cell location on the grid.
#[derive(Component, Hash)]
#[require(
    Button,
    Replicated,
    BackgroundColor(BACKGROUND_COLOR),
    Signature::of::<Cell>(),
    Node {
        width: Val::Px(BUTTON_SIZE),
        height: Val::Px(BUTTON_SIZE),
        margin: UiRect::all(Val::Px(BUTTON_MARGIN)),
        ..Default::default()
    }
)]
struct Cell {
    index: usize,
}

/// Player that can be controlled from the current machine.
#[derive(Component)]
#[require(Replicated)]
struct LocalPlayer;

/// Player that is also a client.
#[derive(Component, Hash)]
#[require(Replicated, Signature::of::<ClientPlayer>())]
struct ClientPlayer;

/// A symbol pick.
#[derive(Event, Deserialize, Serialize, Clone, Copy)]
struct PickCell {
    index: usize,
}
