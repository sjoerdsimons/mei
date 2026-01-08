use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;

// MEI IOCTL definitions
const MEI_IOCTL_MAGIC: u8 = b'H';
const MEI_CONNECT_CLIENT_IOCTL_NUMBER: u8 = 0x01;

// AMT (HECI) Client UUID:  12f80028-b4b7-4b2d-aca8-46e0ff65814c
// This is actually MKHI (Host Interface), the main AMT communication interface
const AMT_UUID: [u8; 16] = [
    0x12, 0xf8, 0x00, 0x28, 0xb4, 0xb7, 0x4b, 0x2d, 0xac, 0xa8, 0x46, 0xe0, 0xff, 0x65, 0x81, 0x4c,
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
#[derive(Debug, Copy, Clone)]
struct MeiConnectClientData {
    in_client_uuid: MeiUuid,
    out_client_properties: MeiClient,
}

// HECI message structures for AMT provisioning state
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct MkhiMessageHeader {
    group_id: u8,
    command: u8,
    is_response: u8,
    reserved: u8,
    result: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct ProvisioningStateRequest {
    header: MkhiMessageHeader,
}

#[repr(C, packed)]
#[derive(Debug)]
struct ProvisioningStateResponse {
    header: MkhiMessageHeader,
    provisioning_state: u32,
}

// MKHI commands
const MKHI_GEN_GROUP_ID: u8 = 0xFF;
const MKHI_GET_FW_VERSION_CMD: u8 = 0x02;

// For provisioning state, we use a different group
const MKHI_FWCAPS_GROUP_ID: u8 = 0x03;
const MKHI_GET_PROVISIONING_STATE_CMD: u8 = 0x11;

// Provisioning states
const PROVISIONING_STATE_PRE: u32 = 0;
const PROVISIONING_STATE_IN: u32 = 1;
const PROVISIONING_STATE_POST: u32 = 2;

// Helper macro for ioctl
macro_rules! iowr {
    ($type:expr, $nr:expr, $size:expr) => {
        (2u32 << 30) | (($size as u32 & 0x1FFF) << 16) | (($type as u32) << 8) | ($nr as u32)
    };
}

fn connect_to_mei_client(file: &File, uuid: &[u8; 16]) -> io::Result<MeiClient> {
    let mut connect_data = MeiConnectClientData {
        in_client_uuid: MeiUuid { data: *uuid },
        out_client_properties: MeiClient {
            max_msg_length: 0,
            protocol_version: 0,
            reserved: [0; 3],
        },
    };

    let ioctl_cmd = iowr!(
        MEI_IOCTL_MAGIC,
        MEI_CONNECT_CLIENT_IOCTL_NUMBER,
        std::mem::size_of::<MeiConnectClientData>()
    );

    unsafe {
        let ret = libc::ioctl(
            file.as_raw_fd(),
            ioctl_cmd as libc::c_ulong,
            &mut connect_data as *mut MeiConnectClientData,
        );

        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(connect_data.out_client_properties)
}

fn get_provisioning_state(mut file: &File) -> io::Result<u32> {
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

    // Send the request
    let request_bytes = unsafe {
        std::slice::from_raw_parts(
            &request as *const _ as *const u8,
            std::mem::size_of::<ProvisioningStateRequest>(),
        )
    };

    file.write_all(request_bytes)?;

    // Read the response
    let mut response_buf = [0u8; 512];
    let bytes_read = file.read(&mut response_buf)?;

    if bytes_read < std::mem::size_of::<ProvisioningStateResponse>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too short",
        ));
    }

    let response = unsafe { &*(response_buf.as_ptr() as *const ProvisioningStateResponse) };

    println!("Response header: {:?}", response.header);

    if response.header.result != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("ME returned error: {}", response.header.result),
        ));
    }

    Ok(response.provisioning_state)
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
    println!("Opening /dev/mei0.. .");

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mei0")?;

    println!("Connecting to AMT/MKHI client...");
    let client = connect_to_mei_client(&file, &AMT_UUID)?;

    println!("Connected!  Client properties:");
    println!("  Max message length: {}", client.max_msg_length);
    println!("  Protocol version: {}", client.protocol_version);

    println!("\nQuerying AMT provisioning state...");
    match get_provisioning_state(&file) {
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
