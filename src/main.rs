use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::time::SystemTime;

use anyhow::Context;
use clap::Parser;
use ipnet::IpNet;
use pocketscion::io_config;
use pocketscion::network::scion::topology::{ScionAs, ScionTopology};
use pocketscion::runtime::{PocketScionRuntime, PocketScionRuntimeBuilder};
use pocketscion::state::SharedPocketScionState;
use scion_proto::address::IsdAsn;
use serde::{Deserialize, Serialize};
use snap_tokens::v0::dummy_snap_token;

/// Pocket SCION Configurator - Configure and run pocketscion simulator with networks from JSON files
#[derive(Parser, Debug)]
#[command(name = "pocketscion-configurator")]
#[command(about = "Configure and run the pocketscion simulator with networks from JSON files", long_about = None)]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config.json")]
    config: String,

    /// Tracing level (trace, debug, info, warn, error)
    #[clap(long = "log", default_value = "info")]
    log_level: tracing::Level,

    /// Path to write the SNAP token file
    #[arg(long = "token-file", default_value = "./snap.token")]
    token_file: String,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(cli.log_level)
        .init();

    tracing::info!("Reading config from: {}", cli.config);
    let config_content = std::fs::read_to_string(&cli.config)
        .context(format!("Failed to read config file: {}", cli.config))?;

    let pocket_scion: PocketScionConfig =
        serde_json::from_str(&config_content).context("Failed to parse config file")?;

    // Build topology from config
    let topology = build_topology_from_config(&pocket_scion.topology)?;

    let _pocket_scion_runtime = {
        tracing::info!("Starting Pocket SCION runtime...");

        let mut system_state = SharedPocketScionState::new(SystemTime::now());
        let io_config = io_config::SharedPocketScionIoConfig::new();

        // Set the topology
        system_state.set_topology(topology.clone());

        // Create SCION Network Access Points (SNAPs) if present
        if let Some(snaps) = &pocket_scion.snaps {
            for snap in snaps {
                let isd_as: IsdAsn = snap.data_plane.isd_as.parse()?;

                // Add a new SNAP to the system state
                let snap_id = system_state.add_snap(isd_as)?;

                // Then add an IO config to declare how this control plane can be reached
                io_config.set_snap_control_addr(snap_id, snap.listening_addr);

                // Add an IO config
                io_config.set_snap_data_plane_addr(snap_id, snap.data_plane.listening_addr);
            }
        }

        // Configure endhost APIs if present
        if let Some(endhost_apis) = &pocket_scion.endhost_apis {
            for api_config in endhost_apis {
                let isds: Vec<IsdAsn> = api_config
                    .isds
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<Vec<_>, _>>()?;
                let endhost_api_id = system_state.add_endhost_api(isds);
                io_config.set_endhost_api_addr(endhost_api_id, api_config.listening_addr);
            }
        }

        // Configure routers if present
        if let Some(routers) = &pocket_scion.routers {
            for router_config in routers {
                let isd_as: IsdAsn = router_config.isd_as.parse()?;
                let interfaces: Vec<NonZeroU16> = router_config
                    .interfaces
                    .iter()
                    .map(|&i| NonZeroU16::new(i).context("Interface ID must be non-zero"))
                    .collect::<Result<Vec<_>, _>>()?;

                let router_id = system_state.add_router(
                    isd_as,
                    interfaces,
                    router_config.snap_data_plane_excludes.clone(),
                    router_config.snap_data_plane_interfaces.clone(),
                );
                io_config.set_router_socket_addr(router_id, router_config.listening_addr);
            }
        }

        // Finally we create the PocketScionRuntime
        let rt: PocketScionRuntime = PocketScionRuntimeBuilder::new()
            .with_system_state(system_state.into_state())
            .with_io_config(io_config.into_state())
            .with_mgmt_listen_addr(pocket_scion.management_listen_addr)
            .start()
            .await
            .context("error starting Pocket SCION runtime")?;

        tracing::info!("Pocket SCION runtime started");

        rt
    };

    tracing::info!("Example SCION testnet setup complete.");

    let token = dummy_snap_token();
    tracing::info!("Dummy SNAP token: {}", token);

    // store token on disk
    std::fs::write(&cli.token_file, token)
        .context(format!("Failed to write SNAP token to {}", cli.token_file))?;
    tracing::info!("Dummy SNAP token written to '{}'", cli.token_file);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received shutdown signal, stopping...");
        }
    }

    Ok(())
}

/// Build a topology from the config structure
fn build_topology_from_config(config: &TopologyConfig) -> anyhow::Result<ScionTopology> {
    let mut topo = ScionTopology::new();

    // Add all ASes
    for as_config in &config.ases {
        let isd_asn: IsdAsn = as_config.isd_as.parse()?;
        if as_config.is_core {
            topo.add_as(ScionAs::new_core(isd_asn))?;
        } else {
            topo.add_as(ScionAs::new(isd_asn))?;
        }
    }

    // Add all links
    for link_str in &config.links {
        topo.add_link(link_str.parse()?)?;
    }

    Ok(topo)
}

#[derive(Debug, Serialize, Deserialize)]
struct PocketScionConfig {
    /// The SCION network topology being simulated
    topology: TopologyConfig,
    /// SCION Network Access Points (SNAP) for the server and client
    #[serde(skip_serializing_if = "Option::is_none")]
    snaps: Option<Vec<SnapConfig>>,
    /// Optional endhost API configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    endhost_apis: Option<Vec<EndhostApiConfig>>,
    /// Optional router configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    routers: Option<Vec<RouterConfig>>,
    /// Management API listen address
    management_listen_addr: SocketAddr,
}

#[derive(Debug, Serialize, Deserialize)]
struct TopologyConfig {
    /// List of ASes in the topology
    ases: Vec<AsConfig>,
    /// List of links between ASes
    links: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AsConfig {
    /// ISD-AS identifier (e.g., "1-11")
    isd_as: String,
    /// Whether this AS is a core AS
    is_core: bool,
}

/// SCION Network Access Point (SNAP) configuration
#[derive(Debug, Serialize, Deserialize)]
struct SnapConfig {
    /// Listening address for the SNAP's control plane
    listening_addr: SocketAddr,
    /// This SNAP's data plane
    data_plane: DataPlaneConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct DataPlaneConfig {
    /// ISD-AS identifier for this data plane
    isd_as: String,
    /// The LAN address this data plane should listen on
    listening_addr: SocketAddr,
    /// The (virtual) IP addresses this data plane can assign to its clients
    address_range: Vec<IpNet>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EndhostApiConfig {
    /// ISDs this endhost API serves
    isds: Vec<String>,
    /// Listening address for the endhost API
    listening_addr: SocketAddr,
}

#[derive(Debug, Serialize, Deserialize)]
struct RouterConfig {
    /// ISD-AS identifier
    isd_as: String,
    /// Interface IDs
    interfaces: Vec<u16>,
    /// Listening address
    listening_addr: SocketAddr,
    /// SNAP data plane exclude addresses
    #[serde(default)]
    snap_data_plane_excludes: Vec<IpNet>,
    /// SNAP data plane interfaces
    #[serde(default)]
    snap_data_plane_interfaces: BTreeMap<String, SocketAddr>,
}
