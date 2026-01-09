use nix::ioctl_readwrite;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

// AMT (IAMTHIF) Client UUID: 12f80028-b4b7-4b2d-aca8-46e0ff65814c
// This is the Intel AMT Host Interface (IAMTHIF) client
const AMT_UUID: [u8; 16] = [
    0x28, 0x00, 0xf8, 0x12, 0xb7, 0xb4, 0x2d, 0x4b, 0xac, 0xa8, 0x46, 0xe0, 0xff, 0x65, 0x81, 0x4c,
];

// AMT State UUID for link state query (from Intel LMS GetAMTStateCommand.h)
const AMT_UUID_LINK_STATE: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct MeiUuid {
    data: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct MeiClient {
    max_msg_length: u32,
    protocol_version: u8,
    reserved: [u8; 3],
}

#[repr(C)]
#[derive(Copy, Clone)]
union MeiConnectClientData {
    in_client_uuid: MeiUuid,
    out_client_properties: MeiClient,
}

ioctl_readwrite!(mei_connect_client, b'H', 0x01, MeiConnectClientData);

// AMT Host Interface message structures
#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct AmtVersion {
    major: u8,
    minor: u8,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct AmtHostIfMsgHeader {
    version: AmtVersion,
    _reserved: u16,
    command: u32,
    length: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct AmtHostIfRespHeader {
    header: AmtHostIfMsgHeader,
    status: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct AmtUnicodeString {
    length: u16,
    string: [u8; AMT_UNICODE_STRING_LEN],
}

#[repr(C, packed)]
#[derive(Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct AmtVersionType {
    description: AmtUnicodeString,
    version: AmtUnicodeString,
}

// AMT protocol version
const AMT_MAJOR_VERSION: u8 = 1;
const AMT_MINOR_VERSION: u8 = 1;

// AMT Host Interface commands
// Verified commands from: https://github.com/mjg59/mei-amt-check
const AMT_HOST_IF_CODE_VERSIONS_REQUEST: u32 = 0x0400001A;
const AMT_HOST_IF_PROVISIONING_MODE_REQUEST: u32 = 0x04000008;
const AMT_HOST_IF_PROVISIONING_STATE_REQUEST: u32 = 0x04000011;

// Official Intel LMS commands from: https://github.com/intel/lms
const AMT_HOST_IF_GET_AMT_STATE_REQUEST: u32 = 0x01000001;
const AMT_HOST_IF_DNS_SUFFIX_REQUEST: u32 = 0x04000036;
const AMT_HOST_IF_LAN_INTERFACE_SETTINGS_REQUEST: u32 = 0x04000048;
const AMT_HOST_IF_FQDN_REQUEST: u32 = 0x04000056;
const AMT_HOST_IF_UNPROVISION_REQUEST: u32 = 0x04000010;

// Experimental commands - sources documented in COMMAND_REFERENCES.md
const AMT_HOST_IF_FEATURES_STATE_REQUEST: u32 = 0x04000017;
const AMT_HOST_IF_PLATFORM_TYPE_REQUEST: u32 = 0x04000019;

// Constants for version parsing
const AMT_BIOS_VERSION_LEN: usize = 65;
const AMT_VERSIONS_NUMBER: usize = 50;
const AMT_UNICODE_STRING_LEN: usize = 20;

// Provisioning states
const PROVISIONING_STATE_PRE: u32 = 0;
const PROVISIONING_STATE_IN: u32 = 1;
const PROVISIONING_STATE_POST: u32 = 2;

// Provisioning modes for unprovision command
const PROVISIONING_MODE_NONE: u32 = 0;
const PROVISIONING_MODE_ENTERPRISE: u32 = 1;

fn connect_to_mei_client(file: &File, uuid: &[u8; 16]) -> io::Result<MeiClient> {
    let mut connect_data = MeiConnectClientData {
        in_client_uuid: MeiUuid { data: *uuid },
    };

    unsafe {
        mei_connect_client(file.as_raw_fd(), &mut connect_data)
            .map_err(|e| io::Error::from_raw_os_error(e as i32))?;

        Ok(connect_data.out_client_properties)
    }
}

// Helper to decode AMT status codes (from Intel LMS StatusCodeDefinitions.h)
fn amt_status_to_string(status: u32) -> &'static str {
    match status {
        0x00 => "SUCCESS",
        0x01 => "INTERNAL_ERROR",
        0x02 => "NOT_READY",
        0x03 => "INVALID_PT_MODE/INVALID_AMT_MODE",
        0x04 => "INVALID_MESSAGE_LENGTH",
        0x10 => "NOT_PERMITTED",
        0x1E => "REQUEST_UNEXPECTED",
        0x20 => "INVALID_PROVISIONING_STATE",
        0x21 => "UNSUPPORTED_OBJECT (command not supported on this AMT version)",
        0x22 => "INVALID_TIME",
        0x23 => "INVALID_INDEX",
        0x24 => "INVALID_PARAMETER",
        0x400 => "DISABLED_BY_POLICY",
        0x803 => "INVALID_COMMAND",
        0x812 => "UNSUPPORTED",
        0x814 => "NOT_FOUND",
        0x815 => "INVALID_CREDENTIALS",
        _ => "UNKNOWN_ERROR",
    }
}

fn send_amt_request(file: &mut File, command: u32) -> io::Result<Vec<u8>> {
    let request = AmtHostIfMsgHeader {
        version: AmtVersion {
            major: AMT_MAJOR_VERSION,
            minor: AMT_MINOR_VERSION,
        },
        _reserved: 0,
        command,
        length: 0,
    };

    file.write_all(request.as_bytes())?;
    file.flush()?;

    let mut response_buf = vec![0u8; 8192];
    let bytes_read = file.read(&mut response_buf)?;

    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "No response from device",
        ));
    }

    response_buf.truncate(bytes_read);
    Ok(response_buf)
}

// Helper function for requests that need to send data (like GetAMTState with UUID parameter)
fn send_amt_request_with_data(file: &mut File, command: u32, data: &[u8]) -> io::Result<Vec<u8>> {
    let header = AmtHostIfMsgHeader {
        version: AmtVersion {
            major: AMT_MAJOR_VERSION,
            minor: AMT_MINOR_VERSION,
        },
        _reserved: 0,
        command,
        length: data.len() as u32,
    };

    file.write_all(header.as_bytes())?;
    file.write_all(data)?;
    file.flush()?;

    let mut response_buf = vec![0u8; 8192];
    let bytes_read = file.read(&mut response_buf)?;

    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "No response from device",
        ));
    }

    response_buf.truncate(bytes_read);
    Ok(response_buf)
}

fn get_provisioning_mode(file: &mut File) -> io::Result<u32> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_PROVISIONING_MODE_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response too short: got {} bytes", response_buf.len()),
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    if response_buf.len() < data_offset + 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response missing provisioning mode data",
        ));
    }

    let mode = u32::from_le_bytes([
        response_buf[data_offset],
        response_buf[data_offset + 1],
        response_buf[data_offset + 2],
        response_buf[data_offset + 3],
    ]);
    Ok(mode)
}

fn get_provisioning_state(file: &mut File) -> io::Result<u32> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_PROVISIONING_STATE_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response too short: got {} bytes", response_buf.len()),
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    if response_buf.len() < data_offset + 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response missing provisioning state data",
        ));
    }

    Ok(response_buf[data_offset] as u32)
}

fn get_code_versions(file: &mut File) -> io::Result<Vec<(String, String)>> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_CODE_VERSIONS_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response too short: got {} bytes", response_buf.len()),
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    let data_len = response_buf.len() - data_offset;

    // Check we have at least BIOS version + count
    if data_len < AMT_BIOS_VERSION_LEN + 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Response too short for version data: got {} bytes, need at least {}",
                data_len,
                AMT_BIOS_VERSION_LEN + 4
            ),
        ));
    }

    // Read count from the data
    let count_offset = data_offset + AMT_BIOS_VERSION_LEN;
    let count = u32::from_le_bytes([
        response_buf[count_offset],
        response_buf[count_offset + 1],
        response_buf[count_offset + 2],
        response_buf[count_offset + 3],
    ]) as usize;

    let mut result = Vec::new();
    let versions_offset = count_offset + 4;

    // Parse each version entry
    for i in 0..count.min(AMT_VERSIONS_NUMBER) {
        let entry_offset = versions_offset + i * std::mem::size_of::<AmtVersionType>();

        if entry_offset + std::mem::size_of::<AmtVersionType>() > response_buf.len() {
            break; // Not enough data for this entry
        }

        let version_type = AmtVersionType::read_from_prefix(&response_buf[entry_offset..])
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Failed to parse version entry")
            })?;

        let desc_len =
            unsafe { std::ptr::addr_of!(version_type.description.length).read_unaligned() }
                as usize;
        let ver_len =
            unsafe { std::ptr::addr_of!(version_type.version.length).read_unaligned() } as usize;

        if desc_len > AMT_UNICODE_STRING_LEN || ver_len > AMT_UNICODE_STRING_LEN {
            continue;
        }

        let description = String::from_utf8_lossy(&version_type.description.string[..desc_len])
            .trim_end_matches('\0')
            .to_string();
        let version = String::from_utf8_lossy(&version_type.version.string[..ver_len])
            .trim_end_matches('\0')
            .to_string();

        result.push((description, version));
    }

    Ok(result)
}

fn get_amt_state(file: &mut File) -> io::Result<Vec<u8>> {
    // GetAMTState requires a UUID parameter (state variable identifier)
    // We use AMT_UUID_LINK_STATE to query link status, crypto fuse, flash protection, etc.
    let mut request_buf = Vec::new();
    request_buf.extend_from_slice(&AMT_UUID_LINK_STATE);

    let response_buf =
        send_amt_request_with_data(file, AMT_HOST_IF_GET_AMT_STATE_REQUEST, &request_buf)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    Ok(response_buf[data_offset..].to_vec())
}

fn get_features_state(file: &mut File) -> io::Result<Vec<u8>> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_FEATURES_STATE_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    Ok(response_buf[data_offset..].to_vec())
}

fn get_platform_type(file: &mut File) -> io::Result<Vec<u8>> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_PLATFORM_TYPE_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    Ok(response_buf[data_offset..].to_vec())
}

fn get_lan_interface_settings(file: &mut File, interface_id: u32) -> io::Result<Vec<u8>> {
    // Send interface ID as part of the request
    let mut request_buf = vec![0u8; std::mem::size_of::<AmtHostIfMsgHeader>() + 4];

    let header = AmtHostIfMsgHeader {
        version: AmtVersion {
            major: AMT_MAJOR_VERSION,
            minor: AMT_MINOR_VERSION,
        },
        _reserved: 0,
        command: AMT_HOST_IF_LAN_INTERFACE_SETTINGS_REQUEST,
        length: 4, // Size of interface_id
    };

    // Copy header
    request_buf[..std::mem::size_of::<AmtHostIfMsgHeader>()].copy_from_slice(header.as_bytes());

    // Copy interface ID in little-endian
    let offset = std::mem::size_of::<AmtHostIfMsgHeader>();
    request_buf[offset..offset + 4].copy_from_slice(&interface_id.to_le_bytes());

    // Send request
    file.write_all(&request_buf)?;
    file.flush()?;

    // Read response
    let mut response_buf = vec![0u8; 8192];
    let bytes_read = file.read(&mut response_buf)?;

    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "No response from device",
        ));
    }

    response_buf.truncate(bytes_read);

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    Ok(response_buf[data_offset..].to_vec())
}

fn get_fqdn(file: &mut File) -> io::Result<String> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_FQDN_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    let fqdn_data = &response_buf[data_offset..];

    let fqdn = String::from_utf8_lossy(fqdn_data)
        .trim_end_matches('\0')
        .to_string();

    Ok(fqdn)
}

fn get_dns_suffix(file: &mut File) -> io::Result<String> {
    let response_buf = send_amt_request(file, AMT_HOST_IF_DNS_SUFFIX_REQUEST)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    let dns_data = &response_buf[data_offset..];

    // DNS suffix is typically a null-terminated string
    let suffix = String::from_utf8_lossy(dns_data)
        .trim_end_matches('\0')
        .to_string();

    Ok(suffix)
}

fn unprovision_amt(file: &mut File, mode: u32) -> io::Result<()> {
    // Send unprovision command with the specified mode
    let mode_bytes = mode.to_le_bytes();
    let response_buf =
        send_amt_request_with_data(file, AMT_HOST_IF_UNPROVISION_REQUEST, &mode_bytes)?;

    if response_buf.len() < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "AMT error 0x{:08x}: {}",
                status,
                amt_status_to_string(status)
            ),
        ));
    }

    Ok(())
}

fn provisioning_state_to_string(state: u32) -> &'static str {
    match state {
        PROVISIONING_STATE_PRE => "Pre-provisioning (not configured)",
        PROVISIONING_STATE_IN => "In provisioning (being configured)",
        PROVISIONING_STATE_POST => "Post-provisioning (configured)",
        _ => "Unknown state",
    }
}

fn provisioning_mode_to_string(mode: u32) -> &'static str {
    match mode {
        0 => "None",
        1 => "Enterprise",
        2 => "Small Business",
        _ => "Unknown mode",
    }
}

fn print_usage() {
    eprintln!("Intel AMT/MEI Query Tool");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  mei                          Query AMT information (default)");
    eprintln!("  mei unprovision <mode>       Unprovision AMT with specified mode");
    eprintln!();
    eprintln!("UNPROVISION MODES:");
    eprintln!("  0 or none        Unprovision to none mode");
    eprintln!("  1 or enterprise  Unprovision to enterprise mode");
    eprintln!();
    eprintln!("EXAMPLES:");
    eprintln!("  mei                    # Query AMT status");
    eprintln!("  mei unprovision 0      # Unprovision to none mode");
    eprintln!("  mei unprovision 1      # Unprovision to enterprise mode");
    eprintln!();
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Parse command line arguments
    if args.len() > 1 {
        match args[1].as_str() {
            "help" | "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            "unprovision" => {
                if args.len() < 3 {
                    eprintln!("Error: unprovision command requires a mode argument");
                    eprintln!();
                    print_usage();
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Missing mode argument",
                    ));
                }

                let mode = match args[2].as_str() {
                    "0" | "none" => PROVISIONING_MODE_NONE,
                    "1" | "enterprise" => PROVISIONING_MODE_ENTERPRISE,
                    _ => {
                        eprintln!("Error: Invalid mode '{}'", args[2]);
                        eprintln!();
                        print_usage();
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid mode"));
                    }
                };

                println!("Opening /dev/mei0...");
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open("/dev/mei0")?;

                println!("Connecting to AMT/IAMTHIF client...");
                let _client = connect_to_mei_client(&file, &AMT_UUID)?;

                // Check current provisioning state first
                print!("Checking current provisioning state... ");
                match get_provisioning_state(&mut file) {
                    Ok(state) => {
                        println!("{}", provisioning_state_to_string(state));

                        if state != PROVISIONING_STATE_POST {
                            eprintln!();
                            eprintln!(
                                "✗ Likely cannot unprovision: AMT is not in post-provisioning state"
                            );
                            eprintln!(
                                "  Current state: {} - {}",
                                state,
                                provisioning_state_to_string(state)
                            );
                            eprintln!();
                            eprintln!(
                                "  The unprovision command likely only works when AMT is already"
                            );
                            eprintln!(
                                "  provisioned (post-provisioning state). Your AMT is currently"
                            );
                            eprintln!("  in pre-provisioning or in-provisioning state.");
                        }
                    }
                    Err(e) => {
                        eprintln!("failed");
                        eprintln!("✗ Could not query provisioning state: {}", e);
                        return Err(e);
                    }
                }

                println!(
                    "\nUnprovisioning AMT with mode {} ({})...",
                    mode,
                    provisioning_mode_to_string(mode)
                );

                match unprovision_amt(&mut file, mode) {
                    Ok(()) => {
                        println!("✓ Unprovision command completed successfully");
                        println!(
                            "AMT has been unprovisioned to {} mode",
                            provisioning_mode_to_string(mode)
                        );
                    }
                    Err(e) => {
                        eprintln!("✗ Unprovision command failed: {}", e);
                        return Err(e);
                    }
                }

                return Ok(());
            }
            _ => {
                eprintln!("Error: Unknown command '{}'", args[1]);
                eprintln!();
                print_usage();
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unknown command",
                ));
            }
        }
    }

    // Default: query mode
    println!("Opening /dev/mei0...");

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mei0")?;

    println!("Connecting to AMT/IAMTHIF client...");
    let client = connect_to_mei_client(&file, &AMT_UUID)?;

    println!("Connected! Client properties:");
    println!("  Max message length: {}", client.max_msg_length);
    println!("  Protocol version: {}", client.protocol_version);

    println!("\n=== AMT Information ===");

    match get_provisioning_state(&mut file) {
        Ok(state) => {
            println!(
                "Provisioning State: {} - {}",
                state,
                provisioning_state_to_string(state)
            );
        }
        Err(e) => eprintln!("Failed to get provisioning state: {}", e),
    }

    match get_provisioning_mode(&mut file) {
        Ok(mode) => {
            println!(
                "Provisioning Mode: {} - {}",
                mode,
                provisioning_mode_to_string(mode)
            );
        }
        Err(e) => eprintln!("Failed to get provisioning mode: {}", e),
    }

    match get_code_versions(&mut file) {
        Ok(versions) => {
            println!("\nFirmware Versions:");
            for (description, version) in versions {
                println!("  {}: {}", description, version);
            }
        }
        Err(e) => eprintln!("\nFailed to get code versions: {}", e),
    }

    // Try to get additional information (may not be supported on all AMT versions)
    println!("\n=== Additional AMT Information ===");

    match get_platform_type(&mut file) {
        Ok(data) if !data.is_empty() => {
            println!("Platform Type: {:02x?}", data);
        }
        Ok(_) => println!("Platform Type: Not available"),
        Err(_) => println!("Platform Type: Not supported"),
    }

    match get_features_state(&mut file) {
        Ok(data) if !data.is_empty() => {
            println!("Features State: {} bytes of data", data.len());
            if data.len() >= 4 {
                let features = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                println!("  Features bitmask: 0x{:08x}", features);
                if features & 0x01 != 0 {
                    println!("  - Redirection enabled");
                }
                if features & 0x02 != 0 {
                    println!("  - KVM enabled");
                }
                if features & 0x04 != 0 {
                    println!("  - Serial over LAN enabled");
                }
            }
        }
        Ok(_) => println!("Features State: Not available"),
        Err(_) => println!("Features State: Not supported"),
    }

    match get_fqdn(&mut file) {
        Ok(fqdn) if !fqdn.is_empty() => {
            println!("Fully Qualified Domain Name: {}", fqdn);
        }
        Ok(_) => println!("Fully Qualified Domain Name: Not configured"),
        Err(_) => println!("Fully Qualified Domain Name: Not supported"),
    }

    match get_dns_suffix(&mut file) {
        Ok(suffix) if !suffix.is_empty() => {
            println!("DNS Suffix: {}", suffix);
        }
        Ok(_) => println!("DNS Suffix: Not configured"),
        Err(_) => println!("DNS Suffix: Not supported"),
    }

    match get_lan_interface_settings(&mut file, 0) {
        Ok(data) if !data.is_empty() => {
            println!("\nLAN Interface Settings (interface 0):");
            println!("  Raw data ({} bytes): {:02x?}", data.len(), data);

            // Parse LAN_SETTINGS structure based on Intel LMS
            // struct LAN_SETTINGS {
            //     AMT_BOOLEAN Enabled;        // 4 bytes (uint32_t)
            //     CFG_IPv4_ADDRESS Ipv4Address; // 4 bytes (uint32_t)
            //     AMT_BOOLEAN DhcpEnabled;    // 4 bytes (uint32_t)
            //     uint8_t DhcpIpMode;         // 1 byte
            //     uint8_t LinkStatus;         // 1 byte
            //     uint8_t MacAddress[6];      // 6 bytes
            // }; // Total: 20 bytes

            if data.len() >= 20 {
                let enabled = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                println!("  Enabled: {}", enabled != 0);

                let ipv4 = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                let ip_bytes = ipv4.to_be_bytes(); // Network byte order for display
                println!(
                    "  IPv4 Address: {}.{}.{}.{}",
                    ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]
                );

                let dhcp_enabled = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                println!("  DHCP Enabled: {}", dhcp_enabled != 0);

                let dhcp_ip_mode = data[12];
                println!("  DHCP IP Mode: 0x{:02x}", dhcp_ip_mode);

                let link_status = data[13];
                println!(
                    "  Link Status: 0x{:02x} ({})",
                    link_status,
                    match link_status {
                        0 => "Down",
                        1 => "Up",
                        _ => "Unknown",
                    }
                );

                println!(
                    "  MAC Address: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    data[14], data[15], data[16], data[17], data[18], data[19]
                );
            } else {
                println!("  Warning: Response too short for full LAN_SETTINGS structure");
                println!("  Expected at least 20 bytes, got {}", data.len());
            }
        }
        Ok(_) => println!("\nLAN Interface Settings: Not available"),
        Err(e) => println!("\nLAN Interface Settings: Not supported ({})", e),
    }

    // Query AMT State (link status, crypto fuse, flash protection, ME reset type)
    // Note: This command is not supported on AMT 9.1.x and will return error 0x21
    match get_amt_state(&mut file) {
        Ok(data) if data.len() >= 21 => {
            // Response structure: 16 bytes UUID + 4 bytes array size + 5 bytes state data
            println!("\nAMT State:");

            // Skip the returned UUID (16 bytes) and array size (4 bytes)
            let state_data = &data[20..];

            if state_data.len() >= 5 {
                let link_status = state_data[0];
                let crypto_fuse = state_data[2];
                let flash_protection = state_data[3];
                let last_me_reset = state_data[4];

                println!(
                    "  Link Status: {}",
                    if link_status == 1 { "Up" } else { "Down" }
                );
                println!(
                    "  Crypto Fuse: {}",
                    if crypto_fuse == 1 {
                        "Enabled"
                    } else {
                        "Disabled"
                    }
                );
                println!(
                    "  Flash Protection: {}",
                    if flash_protection == 1 {
                        "Enabled"
                    } else {
                        "Disabled"
                    }
                );
                println!(
                    "  Last ME Reset: {}",
                    match last_me_reset {
                        0 => "None",
                        1 => "ME Reset",
                        2 => "Global Reset",
                        3 => "Exception",
                        _ => "Unknown",
                    }
                );

                // Show raw state data for debugging
                println!("  Raw state data: {:02x?}", state_data);
            }
        }
        Ok(_) => println!("\nAMT State: Response too short or invalid"),
        Err(e) => eprintln!("\nFailed to get AMT state: {}", e),
    }

    Ok(())
}
