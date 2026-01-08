# AMT Host Interface Command References

## Verified Commands (from mjg59/mei-amt-check)

**Source:** https://github.com/mjg59/mei-amt-check/blob/master/mei-amt-check.c

These commands are verified working and documented in the reference implementation:

| Command | Value | Description | Status |
|---------|-------|-------------|--------|
| `AMT_HOST_IF_CODE_VERSIONS_REQUEST` | `0x0400001A` | Get firmware version information | ✅ Verified |
| `AMT_HOST_IF_PROVISIONING_MODE_REQUEST` | `0x04000008` | Get provisioning mode (None/Enterprise/Small Business) | ✅ Verified |
| `AMT_HOST_IF_PROVISIONING_STATE_REQUEST` | `0x04000011` | Get provisioning state (pre/in/post) | ✅ Verified |

## Official Intel LMS Commands

**Source:** https://github.com/intel/lms (Intel's official Local Manageability Service)

These commands are from Intel's official open-source implementation:

| Command | Value | Description | Header File | Status |
|---------|-------|-------------|-------------|--------|
| `AMT_HOST_IF_CODE_VERSIONS_REQUEST` | `0x0400001A` | Get firmware version information | GetCodeVersionCommand.h | ✅ Verified |
| `AMT_HOST_IF_PROVISIONING_STATE_REQUEST` | `0x04000011` | Get provisioning state | GetProvisioningStateCommand.h | ✅ Verified |
| `AMT_HOST_IF_DNS_SUFFIX_REQUEST` | `0x04000036` | Get DNS suffix | GetDNSSuffixCommand.h | ✅ Official |
| `AMT_HOST_IF_LAN_INTERFACE_SETTINGS_REQUEST` | `0x04000048` | Get LAN interface settings | GetLanInterfaceSettingsCommand.h | ✅ Official |
| `AMT_HOST_IF_FQDN_REQUEST` | `0x04000056` | Get fully qualified domain name | GetFQDNCommand.h | ✅ Official |

### LAN Interface Settings Structure

Based on the Intel LMS implementation (GetLanInterfaceSettingsCommand.h):

```cpp
struct LAN_SETTINGS {
    AMT_BOOLEAN Enabled;              // 1 byte
    CFG_IPv4_ADDRESS Ipv4Address;     // 4 bytes
    AMT_BOOLEAN DhcpEnabled;          // 1 byte
    uint8_t DhcpIpMode;               // 1 byte
    uint8_t LinkStatus;               // 1 byte
    uint8_t MacAddress[6];            // 6 bytes
};
```

Note: `AMT_HOST_IF_LAN_INTERFACE_SETTINGS_REQUEST` requires an interface ID parameter (typically 0 for the first interface).

**Warning:** These commands are based on various Intel AMT documentation and reverse engineering efforts. They may not work on all AMT versions or may require specific provisioning states.

### Sources:
1. **Intel LMS (Local Manageability Service)** - Official Intel open-source implementation
   - https://github.com/intel/lms
   - Apache 2.0 licensed
   - Most reliable source for command codes

2. **mjg59/mei-amt-check** - Verified working minimal implementation
   - https://github.com/mjg59/mei-amt-check
   - Used as reference for basic commands

3. **Community reverse engineering:**
   - https://github.com/Ylianst/MeshCommander
   - Various security research on Intel ME/AMT

### Experimental Command List:

| Command | Value | Description | Source | Status |
|---------|-------|-------------|--------|--------|
| `AMT_HOST_IF_FEATURES_STATE_REQUEST` | `0x04000017` | Get enabled features bitmask | Intel SDK docs | ⚠️ Experimental |
| `AMT_HOST_IF_PLATFORM_TYPE_REQUEST` | `0x04000019` | Get platform type information | Intel SDK docs | ⚠️ Experimental |
| `AMT_HOST_IF_LAN_INTERFACE_SETTINGS_REQUEST` | `0x04000020` | Get LAN/network settings | Community tools | ⚠️ Experimental |
| `AMT_HOST_IF_DNS_SUFFIX_REQUEST` | `0x04000022` | Get DNS suffix configuration | Community tools | ⚠️ Experimental |

## Command Pattern

All AMT Host Interface commands follow this pattern:
- Request commands: `0x04XXXXXX` (bit 23 = 0)
- Response commands: `0x048XXXXX` (bit 23 = 1)

The response command code is always request command OR'd with `0x00800000`.

## Protocol Documentation

### Official Intel Resources:
- **Intel AMT Implementation and Reference Guide** (various versions)
  - Typically available through Intel's Resource & Design Center
  - Requires NDA or partner agreement for detailed HECI/MEI specs

### Community Resources:
- **Linux MEI Driver:** https://www.kernel.org/doc/html/latest/driver-api/mei/index.html
- **coreboot AMT documentation:** https://doc.coreboot.org/
- **Reverse engineering notes:** Various security research papers on Intel ME/AMT

## Known Limitations

1. **Version-specific:** Different AMT versions (6.x through 16.x) support different commands
2. **Provisioning state:** Some commands only work in specific provisioning states
3. **Authentication:** Advanced commands may require authenticated sessions
4. **Undocumented:** Intel does not publicly document all HECI/MEI commands

## Feature Bits (0x04000017 response)

Based on reverse engineering and community documentation:

| Bit | Feature | Description |
|-----|---------|-------------|
| 0 | Redirection | IDE-R, SOL, KVM redirection |
| 1 | KVM | Keyboard/Video/Mouse remote control |
| 2 | SOL | Serial Over LAN |
| 3 | Reserved | Unknown/varies by version |

**Note:** Bit definitions may vary by AMT version and are not officially documented.

## Additional Reading

1. **"Disabling Intel ME 11 via undocumented mode"** - Security research on ME/AMT
   - https://github.com/corna/me_cleaner

2. **"Intel ME: The Way of the Static Analysis"** - Trammell Hudson's research
   - Various security conference presentations

3. **Linux kernel HECI/MEI documentation**
   - Best source for understanding the MEI transport layer

## Disclaimer

The experimental commands are provided for research and diagnostic purposes. They are:
- Not officially documented by Intel
- May not work on all systems
- May return different data formats across AMT versions
- Implemented with graceful failure handling

Always test on non-production systems first.
