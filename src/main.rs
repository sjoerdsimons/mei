use nix::ioctl_readwrite;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

// AMT (IAMTHIF) Client UUID: 12f80028-b4b7-4b2d-aca8-46e0ff65814c
// This is the Intel AMT Host Interface (IAMTHIF) client
const AMT_UUID: [u8; 16] = [
    0x28, 0x00, 0xf8, 0x12, 0xb7, 0xb4, 0x2d, 0x4b, 0xac, 0xa8, 0x46, 0xe0, 0xff, 0x65, 0x81, 0x4c,
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
const AMT_HOST_IF_CODE_VERSIONS_REQUEST: u32 = 0x0400001A;
const AMT_HOST_IF_PROVISIONING_MODE_REQUEST: u32 = 0x04000008;
const AMT_HOST_IF_PROVISIONING_STATE_REQUEST: u32 = 0x04000011;

// Constants for version parsing
const AMT_BIOS_VERSION_LEN: usize = 65;
const AMT_VERSIONS_NUMBER: usize = 50;
const AMT_UNICODE_STRING_LEN: usize = 20;

// Provisioning states
const PROVISIONING_STATE_PRE: u32 = 0;
const PROVISIONING_STATE_IN: u32 = 1;
const PROVISIONING_STATE_POST: u32 = 2;

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

    use std::io::Write;
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
            format!("AMT returned error status: 0x{:08x}", status),
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
            format!("AMT returned error status: 0x{:08x}", status),
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
            format!("AMT returned error status: 0x{:08x}", status),
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

fn main() -> io::Result<()> {
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

    Ok(())
}
