# Pocket SCION Configurator

A wrapper around the [pocketscion network simulator](https://github.com/Anapaya/scion-sdk/tree/main/pocketscion) that allows configuration via JSON files.


## Intended Use
This tool is designed to simplify the setup and configuration of local SCION networks using the pocketscion simulator. It allows users to define network topologies, SNAPs, endhost APIs, and routers as JSON files.

The network is configured and started as a single process. Applications can connect to the simulated SCION network via the configured SNAPs or Endhost API addresses. Combined with network namespaces this can be used as an out-of-the-box SCION test environment for applications. We provide one such setup with `namespace.sh` and `namespace_config.json`. It is described below.

A further advantage of using config files is that changes to the topology do not require recompilation of the simulator.

I use it for both local testing and as part of CI integration tests for [connect-ip-rust-scion](https://github.com/aaronbojarski/connect-ip-rust-scion).


## Network Namespace Setup
To isolate the simulated network, you can use Linux network namespaces.
`namespace.sh` is a helper script that sets up three network namespaces. It can be used in conjunction with namespace_config.json. It will set up the following network.

```
┌─────────────────────────┐         ┌──────────────────────────────────────┐         ┌─────────────────────────┐
│   Client Namespace      │         │    Simulator Namespace               │         │   Server Namespace      │
│                         │         │                                      │         │                         │
│  ┌──────────────────┐   │         │   ┌──────────────────────────────┐   │         │   ┌──────────────────┐  │
│  │  Client App      │   │         │   │   Pocket SCION Simulator     │   │         │   │   Server App     │  │
│  │                  │   │         │   │                              │   │         │   │                  │  │
│  └────────┬─────────┘   │         │   │  ┌────────────────────────┐  │   │         │   └─────────┬────────┘  │
│           └─────────────┼─────────┼───┼─►│  Client SNAP           │  │   │         │             │           │
│             Connect via │         │   │  │  ISD-AS: 1-4           │  │   │         │             │           │
│             SNAP        │         │   │  └────────────────────────┘  │   │         │             │           │
│                         │         │   │                              │   │         │ Connect via │           │
│                         │         │   │  ┌────────────────────────┐  │   │         │ Endhost API │           │
│                         │         │   │  │  Server SNAP           │  │   │         │             │           │
│                         │         │   │  │  ISD-AS: 2-4           │  │   │         │             │           │
│  10.0.100.10            │         │   │  └────────────────────────┘  │   │         │  10.0.200.10            │
│  (veth-client)          │         │   │                              │   │         │  (veth-server)          │
│                         │         │   │  ┌────────────────────────┐  │   │         │             │           │
│                         │         │   │  │  Client Endhost API    │  │   │         │             │           │
│                         │         │   │  │  ISD-AS: 1-3           │  │   │         │             │           │
└─────────┬───────────────┘         │   │  └────────────────────────┘  │   │         └─────────────┼─────┬─────┘
          │                         │   │                              │   │                       │     │
          │                         │   │  ┌────────────────────────┐  │   │                       │     │
          │                         │   │  │  Server Endhost API    │◄─┼───┼───────────────────────┘     │
          │   10.0.100.0/24         │   │  │  ISD-AS: 2-3           │  │   │                             │
          │   network link          │   │  └────────────────────────┘  │   │         10.0.200.0/24       │
          └─────────────────────────┤   │                              │   │            network link     │
                                    │   │  10.0.100.20 (left iface)    │   ├─────────────────────────────┘
                                    │   │  10.0.200.20 (right iface)   │   │
                                    │   │                              │   │
                                    │   └──────────────────────────────┘   │
                                    │                                      │
                                    └──────────────────────────────────────┘

Connections:
  • Client apps connect to Client SNAP: 10.0.100.20:10142 (control) / :10143 (data)
  • Server apps connect to Server Endhost API: 10.0.200.20:10231
  • Alternative: Client Endhost API at 10.0.100.20:10131, Server SNAP at 10.0.200.20:10242/:10243
  • Management API available at 127.0.0.1:8082 (within simulator namespace)
```


### Usage
1. Build the project with cargo.
  ```bash
  cargo build
  ```

2. Set up the namespaces.
```bash
sudo bash ./namespace.sh up
```

3. Run the simulator within the simulator namespace.
```bash
sudo ip netns exec pocketscion_ns ./target/debug/pocketscion-configurator -c ./namespace_config.json
```

4. Connect your SCION applications to the configured SNAPs or Endhost APIs.
```bash
sudo ip netns exec server_ns ./your_scion_server_app --endhost-api-addr 10.0.200.20:10231
```
```bash
sudo ip netns exec client_ns ./your_scion_client_app --snap-addr 10.0.100.20:10142
```

5. Tear down the namespaces when done.
```bash
sudo bash ./namespace.sh down
```

### SNAP Token File

When the simulator starts, it generates a `snap.token` file in the current working directory (or at a path specified via the `--token-file` CLI argument). This file contains a currently valid authentication token that can be used to authenticate with the SNAPs in the configured network. Your client applications will need to use this token when connecting to SNAP endpoints.


## Configuration File Format
The configuration file is a JSON file that defines the network topology, SNAPs, endhost APIs, and routers. Note that currently we do not perform any validation of the configuration file beyond basic JSON syntax checking. The user needs to ensure that address and port assignments do not conflict.

### Example Config
The following shows a minimal example configuration that can be used without namespaces.

```json
{
  "topology": {
    "ases": [
      { "isd_as": "1-1", "is_core": true },
      { "isd_as": "1-2", "is_core": false }
    ],
    "links": [
      "1-1#1 down_to 1-2#2"
    ]
  },
  "snaps": [
    {
      "listening_addr": "127.0.0.1:10122",
      "data_planes": [
        {
          "isd_as": "1-2",
          "listening_addr": "127.0.0.1:10123",
          "address_range": ["10.1.0.0/24"]
        }
      ]
    }
  ],
  "endhost_apis": [
    {
      "isds": ["1-1"],
      "listening_addr": "127.0.0.1:10111"
    }
  ],
  "routers": [
    {
      "isd_as": "1-1",
      "interfaces": [1],
      "local_addresses": [],
      "next_hops": {}
    }
  ],
  "management_listen_addr": "127.0.0.1:8082"
}
```

### Configuration Sections

#### Topology

Defines the SCION network topology being simulated.

- **ases**: Array of Autonomous Systems
  - `isd_as`: ISD-AS identifier (e.g., "1-11")
  - `is_core`: Whether this AS is a core AS (boolean)

- **links**: Array of link definitions as strings
  - Format: `"<AS1>#<interface> <type> <AS2>#<interface>"`
  - Link types: `core`, `down_to`
  - Example: `"1-1#5 core 1-11#6"`

#### SNAPs

Defines SCION Network Access Points (SNAPs) for clients to connect.

- **name**: Descriptive name for the SNAP
- **listening_addr**: Control plane listening address (IP:port)
- **data_planes**: Array of data plane configurations
  - `isd_as`: ISD-AS this data plane serves
  - `listening_addr`: Data plane listening address
  - `address_range`: Array of IP ranges (CIDR notation) the data plane can assign

#### Endhost APIs (Optional)

Defines endhost API endpoints.

- **isds**: Array of ISD-AS identifiers this API serves
- **listening_addr**: API listening address

#### Routers (Optional)

Defines router configurations.

- **isd_as**: ISD-AS identifier for the router
- **interfaces**: Array of interface IDs (non-zero integers)
- **local_addresses**: Array of local IP addresses/networks (CIDR)
- **next_hops**: Map of interface IDs (as strings) to next-hop addresses

#### Management Listen Address (Optional)

Management API listening address.
