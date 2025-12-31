use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::atomic::{AtomicU16, Ordering},
};

use bevy::{prelude::*, state::app::StatesPlugin};
use bevy_replicon::prelude::*;
use bevy_replicon_matchbox::{MatchboxClient, MatchboxHost, RepliconMatchboxPlugins};
use serde::{Deserialize, Serialize};
use test_log::test;

//run the tests with cargo test -- --test-threads=1

static PORT_COUNTER: AtomicU16 = AtomicU16::new(30000);
fn next_test_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::AcqRel)
}

#[test]
fn connect_disconnect() {
    let port = next_test_port();
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
            RepliconMatchboxPlugins,
        ))
        .finish();
    }

    setup(&mut server_app, &mut client_app, port);

    let server_state = server_app.world().resource::<State<ServerState>>();
    assert_eq!(*server_state, ServerState::Running);

    let mut clients = server_app
        .world_mut()
        .query::<(&ConnectedClient, &AuthorizedClient)>();
    assert_eq!(clients.iter(server_app.world()).len(), 1);

    let client_state = client_app.world().resource::<State<ClientState>>();
    assert_eq!(*client_state, ClientState::Connected);

    let renet_client = client_app.world().resource::<MatchboxClient>();
    assert!(renet_client.is_connected());

    client_app.world_mut().remove_resource::<MatchboxClient>();

    client_app.update();
    server_app.update();

    assert_eq!(clients.iter(server_app.world()).len(), 0);

    let client_state = client_app.world().resource::<State<ClientState>>();
    assert_eq!(*client_state, ClientState::Disconnected);
}

#[test]
fn disconnect_request() {
    let port = next_test_port();

    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
            RepliconMatchboxPlugins,
        ))
        .add_server_message::<Test>(Channel::Ordered)
        .finish();
    }

    setup(&mut server_app, &mut client_app, port);

    server_app.world_mut().spawn(Replicated);
    server_app.world_mut().write_message(ToClients {
        mode: SendMode::Broadcast,
        message: Test,
    });

    let mut clients = server_app
        .world_mut()
        .query_filtered::<Entity, With<ConnectedClient>>();
    let client = clients.single(server_app.world()).unwrap();
    server_app
        .world_mut()
        .write_message(DisconnectRequest { client });

    server_app.update();
    client_app.update();

    assert_eq!(clients.iter(server_app.world()).len(), 0);

    let client_state = client_app.world().resource::<State<ClientState>>();
    assert_eq!(*client_state, ClientState::Disconnected);

    let messages = client_app.world().resource::<Messages<Test>>();
    assert_eq!(messages.len(), 1, "last message should be received");

    let mut replicated = client_app.world_mut().query::<&Replicated>();
    assert_eq!(
        replicated.iter(client_app.world()).len(),
        1,
        "last replication should be received"
    );
}

#[test]
fn server_stop() {
    let port = next_test_port();

    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
            RepliconMatchboxPlugins,
        ))
        .add_server_message::<Test>(Channel::Ordered)
        .finish();
    }

    setup(&mut server_app, &mut client_app, port);

    server_app.world_mut().remove_resource::<MatchboxHost>();
    server_app.world_mut().spawn(Replicated);
    server_app.world_mut().write_message(ToClients {
        mode: SendMode::Broadcast,
        message: Test,
    });

    server_app.update();
    client_app.update();

    let mut clients = server_app.world_mut().query::<&ConnectedClient>();
    assert_eq!(clients.iter(server_app.world()).len(), 0);

    let server_state = server_app.world().resource::<State<ServerState>>();
    assert_eq!(*server_state, ServerState::Stopped);

    let client_state = client_app.world().resource::<State<ClientState>>();
    assert_eq!(*client_state, ClientState::Disconnected);

    let messages = client_app.world().resource::<Messages<Test>>();
    assert!(
        messages.is_empty(),
        "message shouldn't be received after stop"
    );

    let mut replicated = client_app.world_mut().query::<&Replicated>();
    assert_eq!(
        replicated.iter(client_app.world()).len(),
        0,
        "replication after stop shouldn't be received"
    );
}

#[test]
fn replication() {
    let port = next_test_port();

    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
            RepliconMatchboxPlugins,
        ))
        .finish();
    }

    setup(&mut server_app, &mut client_app, port);

    server_app.world_mut().spawn(Replicated);

    server_app.update();
    client_app.update();

    let mut replicated = client_app.world_mut().query::<&Replicated>();
    assert_eq!(replicated.iter(client_app.world()).len(), 1);
}

#[test]
fn server_message() {
    let port = next_test_port();

    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
            RepliconMatchboxPlugins,
        ))
        .add_server_message::<Test>(Channel::Ordered)
        .finish();
    }

    setup(&mut server_app, &mut client_app, port);

    server_app.world_mut().write_message(ToClients {
        mode: SendMode::Broadcast,
        message: Test,
    });

    server_app.update();
    client_app.update();

    let messages = client_app.world().resource::<Messages<Test>>();
    assert_eq!(messages.len(), 1);
}

#[test]
fn client_message() {
    let port = next_test_port();

    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
            RepliconMatchboxPlugins,
        ))
        .add_client_message::<Test>(Channel::Ordered)
        .finish();
    }

    setup(&mut server_app, &mut client_app, port);

    client_app.world_mut().write_message(Test);

    client_app.update();
    server_app.update();

    let messages = server_app.world().resource::<Messages<FromClient<Test>>>();
    assert_eq!(messages.len(), 1);
}
fn setup(server_app: &mut App, client_app: &mut App, port: u16) {
    start_signaling_server(server_app, port);
    setup_server(server_app, port);
    setup_client(client_app, port);
    wait_for_connection(server_app, client_app);
}

use bevy_matchbox::matchbox_signaling::SignalingServer;

fn start_signaling_server(server_app: &mut App, port: u16) {
    info!("Starting signaling server");
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let signaling_server = bevy_matchbox::MatchboxServer::from(
        SignalingServer::client_server_builder(addr)
            .on_connection_request(|connection| {
                info!("Connecting: {connection:?}");
                Ok(true) // Allow all connections
            })
            .on_id_assignment(|(socket, id)| info!("{socket} received {id}"))
            .on_host_connected(|id| info!("Host joined: {id}"))
            .on_host_disconnected(|id| info!("Host left: {id}"))
            .on_client_connected(|id| info!("Client joined: {id}"))
            .on_client_disconnected(|id| info!("Client left: {id}"))
            .cors()
            // .trace()
            .build(),
    );
    server_app.insert_resource(signaling_server);
}

fn setup_server(app: &mut App, port: u16) {
    let room_url = format!("ws://localhost:{port}/TestRoom");
    let channels = app.world().resource::<RepliconChannels>();

    let server = MatchboxHost::new(room_url, channels).unwrap();

    app.insert_resource(server);
}

fn setup_client(app: &mut App, port: u16) {
    let room_url = format!("ws://localhost:{port}/TestRoom");
    let channels = app.world().resource::<RepliconChannels>();
    let client = MatchboxClient::new(room_url, channels).unwrap();
    app.insert_resource(client);
}

fn wait_for_connection(server_app: &mut App, client_app: &mut App) {
    loop {
        client_app.update();
        server_app.update();
        let host = server_app.world().resource::<MatchboxHost>();
        let client = client_app.world().resource::<MatchboxClient>();
        if host.connected_clients() > 0 && client.is_connected() {
            break;
        }
    }
}

#[derive(Message, Serialize, Deserialize)]
struct Test;
