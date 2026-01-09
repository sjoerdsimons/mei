# MEI AMT Status Query and Management

*** Note this is a one evening "vibe" coding experiment fully written by co-pilot, do with that as you wish ***

A Rust program to query and manage Intel Active Management Technology (AMT) through the MEI (Management Engine Interface).

## Features

- **Query AMT Status**: View provisioning state, mode, firmware versions, and network configuration
- **Unprovision AMT**: Remove provisioning configuration with specified mode
- Queries AMT provisioning state (pre-provisioning, in-provisioning, or post-provisioning)
- Retrieves AMT provisioning mode (None, Enterprise, Small Business)
- Lists firmware version information for all AMT components
- Displays LAN interface settings (IP, DHCP, MAC address, link status)
- Queries FQDN and DNS suffix
- Uses safe Rust with minimal `unsafe` code
- Properly handles packed structs and alignment issues

## Requirements

- Linux system with Intel AMT
- `/dev/mei0` or `/dev/mei` device available
- MEI kernel module loaded (`mei_me`)

## Dependencies

```toml
[dependencies]
nix = { version = "0.29", features = ["ioctl"] }
zerocopy = { version = "0.7", features = ["derive"] }
```

## Building

```bash
cargo build --release
```

## Usage

### Query AMT Information (default)

```bash
./target/release/mei
```

This will display all available AMT information including provisioning state, firmware versions, network configuration, etc.

### Unprovision AMT

```bash
# Unprovision to none mode
./target/release/mei unprovision 0
./target/release/mei unprovision none

# Unprovision to enterprise mode
./target/release/mei unprovision 1
./target/release/mei unprovision enterprise
```

**Important:** 
- AMT must be in **post-provisioning state** (already configured) to unprovision
- The tool will check the state and provide an error if AMT is not provisioned
- Unprovisioning will remove AMT configuration
- This operation requires appropriate permissions and may require a reboot to take full effect

### Help

```bash
./target/release/mei --help
```

## Example Output

```
Opening /dev/mei0...
Connecting to AMT/IAMTHIF client...
Connected! Client properties:
  Max message length: 128
  Protocol version: 2

=== AMT Information ===
Provisioning State: 0 - Pre-provisioning (not configured)
Provisioning Mode: 0 - None

Firmware Versions:
  AMT: 9.1.37.1002
  Sku: Corporate
  Build Number: 1002
  Recovery Version: 9.1.37.1002
  Recovery Build Num: 1002
  Legacy Mode: False
```

## Technical Details

### Protocol
Uses the Intel AMT Host Interface (IAMTHIF) protocol, not MKHI. The IAMTHIF client UUID is `12f80028-b4b7-4b2d-aca8-46e0ff65814c`.

### Key Implementation Details
- UUID bytes use mixed endianness (first 3 fields are little-endian)
- `MeiConnectClientData` is a union, not a struct
- AMT Host Interface uses 12-byte request headers
- Response parsing handles variable-length data correctly

## References

- Primary implementation based on: [mjg59/mei-amt-check](https://github.com/mjg59/mei-amt-check)
- Command documentation: See `COMMAND_REFERENCES.md` for detailed sources
- Linux MEI driver: [Kernel documentation](https://www.kernel.org/doc/html/latest/driver-api/mei/index.html)

## Documentation

- `README.md` - This file, usage and overview
- `FINDINGS.md` - Technical deep-dive into the implementation and fixes
- `COMMAND_REFERENCES.md` - Detailed documentation of AMT commands and sources
- `CHANGELOG.md` - History of changes and fixes

## License

See LICENSE file.
