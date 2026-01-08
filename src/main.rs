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

// AMT protocol version
const AMT_MAJOR_VERSION: u8 = 1;
const AMT_MINOR_VERSION: u8 = 1;

// AMT Host Interface commands
const AMT_HOST_IF_PROVISIONING_STATE_REQUEST: u32 = 0x04000011;

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

fn get_provisioning_state(file: &mut File) -> io::Result<u32> {
    let request = AmtHostIfMsgHeader {
        version: AmtVersion {
            major: AMT_MAJOR_VERSION,
            minor: AMT_MINOR_VERSION,
        },
        _reserved: 0,
        command: AMT_HOST_IF_PROVISIONING_STATE_REQUEST,
        length: 0,
    };

    use std::io::Write;
    file.write_all(request.as_bytes())?;
    file.flush()?;

    let mut response_buf = [0u8; 512];
    let bytes_read = file.read(&mut response_buf)?;

    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "No response from device",
        ));
    }

    if bytes_read < std::mem::size_of::<AmtHostIfRespHeader>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response too short: got {} bytes", bytes_read),
        ));
    }

    let response = AmtHostIfRespHeader::read_from_prefix(&response_buf[..bytes_read])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse response"))?;

    let status = unsafe { std::ptr::addr_of!(response.status).read_unaligned() };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("AMT returned error status: 0x{:08x}", status),
        ));
    }

    let data_offset = std::mem::size_of::<AmtHostIfRespHeader>();
    if bytes_read < data_offset + 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response missing provisioning state data",
        ));
    }

    Ok(response_buf[data_offset] as u32)
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

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mei0")?;

    println!("Connecting to AMT/IAMTHIF client...");
    let client = connect_to_mei_client(&file, &AMT_UUID)?;

    println!("Connected! Client properties:");
    println!("  Max message length: {}", client.max_msg_length);
    println!("  Protocol version: {}", client.protocol_version);

    println!("\nQuerying AMT provisioning state...");
    let state = get_provisioning_state(&mut file)?;

    println!("\n=== AMT Provisioning State ===");
    println!("State value: {}", state);
    println!("State description: {}", provisioning_state_to_string(state));

    Ok(())
}
