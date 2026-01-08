use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use zerocopy::{AsBytes, FromBytes, FromZeroes};
use nix::ioctl_readwrite;

// AMT (HECI) Client UUID:  12f80028-b4b7-4b2d-aca8-46e0ff65814c
// This is actually MKHI (Host Interface), the main AMT communication interface
// Note: First 3 fields are stored in little-endian format (uuid_le)
const AMT_UUID: [u8; 16] = [
    // 12f80028 in LE: 28 00 f8 12
    0x28, 0x00, 0xf8, 0x12,
    // b4b7 in LE: b7 b4
    0xb7, 0xb4,
    // 4b2d in LE: 2d 4b
    0x2d, 0x4b,
    // Last 8 bytes stay as-is
    0xac, 0xa8, 0x46, 0xe0, 0xff, 0x65, 0x81, 0x4c,
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

// Define the ioctl using nix's macro
ioctl_readwrite!(mei_connect_client, b'H', 0x01, MeiConnectClientData);

// HECI message structures for AMT provisioning state
#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct MkhiMessageHeader {
    group_id: u8,
    command: u8,
    is_response: u8,
    reserved: u8,
    result: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct ProvisioningStateRequest {
    header: MkhiMessageHeader,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
struct ProvisioningStateResponse {
    header: MkhiMessageHeader,
    provisioning_state: u32,
}

// MKHI commands
const MKHI_FWCAPS_GROUP_ID: u8 = 0x03;
const MKHI_GET_PROVISIONING_STATE_CMD: u8 = 0x11;

// Provisioning states
const PROVISIONING_STATE_PRE: u32 = 0;
const PROVISIONING_STATE_IN: u32 = 1;
const PROVISIONING_STATE_POST: u32 = 2;

fn connect_to_mei_client(file: &File, uuid: &[u8; 16]) -> io::Result<MeiClient> {
    let mut connect_data = MeiConnectClientData {
        in_client_uuid: MeiUuid { data: *uuid },
    };

    println!("DEBUG: Union size: {}", std::mem::size_of::<MeiConnectClientData>());

    unsafe {
        mei_connect_client(file.as_raw_fd(), &mut connect_data)
            .map_err(|e| {
                eprintln!("DEBUG: ioctl error code: {}", e);
                io::Error::from_raw_os_error(e as i32)
            })?;
        
        // After the ioctl, the union contains out_client_properties
        Ok(connect_data.out_client_properties)
    }
}

fn get_provisioning_state(file: &mut File) -> io::Result<u32> {
    // Create the request message
    let request = ProvisioningStateRequest {
        header: MkhiMessageHeader {
            group_id: MKHI_FWCAPS_GROUP_ID,
            command: MKHI_GET_PROVISIONING_STATE_CMD,
            is_response: 0,
            reserved: 0,
            result: 0,
        },
    };

    println!("DEBUG: Sending request, size: {}", std::mem::size_of::<ProvisioningStateRequest>());
    
    // Send the request using standard Write trait
    use std::io::Write;
    file.write_all(request.as_bytes())?;
    file.flush()?;
    println!("DEBUG: Wrote {} bytes", request.as_bytes().len());

    // Read the response - will block until data is available
    let mut response_buf = [0u8; 512];
    let bytes_read = file.read(&mut response_buf)?;

    println!("DEBUG: Read {} bytes from device", bytes_read);
    println!("DEBUG: Expected at least {} bytes", std::mem::size_of::<ProvisioningStateResponse>());
    
    if bytes_read > 0 {
        println!("DEBUG: First {} bytes: {:02x?}", bytes_read.min(16), &response_buf[..bytes_read.min(16)]);
    }

    if bytes_read < std::mem::size_of::<ProvisioningStateResponse>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response too short: got {} bytes, need {}", bytes_read, std::mem::size_of::<ProvisioningStateResponse>()),
        ));
    }

    // Use zerocopy to safely read the response
    let response = ProvisioningStateResponse::read_from_prefix(&response_buf[..bytes_read])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    println!("Response header: {:?}", response.header);

    // Read result field safely using raw pointer to avoid alignment issues
    let result = unsafe { std::ptr::addr_of!(response.header.result).read_unaligned() };
    
    if result != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("ME returned error: {}", result),
        ));
    }

    // Read provisioning_state safely using raw pointer
    Ok(unsafe { std::ptr::addr_of!(response.provisioning_state).read_unaligned() })
}

fn provisioning_state_to_string(state: u32) -> &'static str {
    match state {
        PROVISIONING_STATE_PRE => "Pre-provisioning (not configured)",
        PROVISIONING_STATE_IN => "In provisioning (being configured)",
        PROVISIONING_STATE_POST => "Post-provisioning (configured)",
        _ => "Unknown state",
    }
}

fn main() -> io::Result<()> {
    println!("Opening /dev/mei0...");

    // Open in blocking mode (without O_NONBLOCK)
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mei0")?;

    println!("Connecting to AMT/MKHI client...");
    let client = connect_to_mei_client(&file, &AMT_UUID)?;

    println!("Connected! Client properties:");
    println!("  Max message length: {}", client.max_msg_length);
    println!("  Protocol version: {}", client.protocol_version);

    println!("\nQuerying AMT provisioning state...");
    match get_provisioning_state(&mut file) {
        Ok(state) => {
            println!("\n=== AMT Provisioning State ===");
            println!("State value: {}", state);
            println!("State description: {}", provisioning_state_to_string(state));
        }
        Err(e) => {
            eprintln!("Failed to get provisioning state: {}", e);
            return Err(e);
        }
    }

    Ok(())
}
