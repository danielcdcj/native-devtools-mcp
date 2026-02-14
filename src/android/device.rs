use adb_client::server::ADBServer;
use adb_client::server_device::ADBServerDevice;
use adb_client::ADBDeviceExt;
use serde::Serialize;

/// Information about a connected Android device.
#[derive(Debug, Clone, Serialize)]
pub struct AndroidDeviceInfo {
    /// Device serial number (e.g. "emulator-5554" or "XXXXXXXX").
    pub serial: String,
    /// Connection state as reported by ADB (e.g. "device", "offline").
    pub state: String,
}

/// Wrapper around an ADB server device that provides convenient shell access.
pub struct AndroidDevice {
    /// The device serial number.
    pub serial: String,
    device: ADBServerDevice,
}

/// List all devices currently connected to the ADB server.
pub fn list_devices() -> Result<Vec<AndroidDeviceInfo>, String> {
    let mut server = ADBServer::default();
    let devices = server
        .devices()
        .map_err(|e| format!("Failed to list ADB devices: {}", e))?;

    Ok(devices
        .into_iter()
        .map(|d| AndroidDeviceInfo {
            serial: d.identifier,
            state: d.state.to_string(),
        })
        .collect())
}

impl AndroidDevice {
    /// Connect to a specific device by its serial number.
    pub fn connect(serial: &str) -> Result<Self, String> {
        let mut server = ADBServer::default();
        let device = server
            .get_device_by_name(serial)
            .map_err(|e| format!("Failed to connect to device '{}': {}", serial, e))?;

        Ok(Self {
            serial: serial.to_string(),
            device,
        })
    }

    /// Run a single shell command string on the device and return its output.
    pub fn shell(&mut self, command: &str) -> Result<String, String> {
        let mut output = Vec::new();
        self.device
            .shell_command(&command, Some(&mut output), None)
            .map_err(|e| format!("Shell command failed: {}", e))?;

        String::from_utf8(output).map_err(|e| format!("Shell output is not valid UTF-8: {}", e))
    }

    /// Run a shell command with multiple arguments on the device.
    ///
    /// The arguments are joined with spaces into a single command string.
    pub fn shell_args(&mut self, args: &[&str]) -> Result<String, String> {
        let command = args.join(" ");
        self.shell(&command)
    }

    /// Run a shell command and capture raw bytes output.
    ///
    /// The arguments are joined with spaces into a single command string.
    pub fn shell_bytes(&mut self, args: &[&str], output: &mut Vec<u8>) -> Result<(), String> {
        let command = args.join(" ");
        self.device
            .shell_command(&command, Some(output), None)
            .map_err(|e| format!("Shell command failed: {}", e))?;
        Ok(())
    }

    /// Capture the device framebuffer as PNG bytes.
    pub fn framebuffer_png(&mut self) -> Result<Vec<u8>, String> {
        self.device
            .framebuffer_bytes()
            .map_err(|e| format!("Failed to capture framebuffer: {}", e))
    }
}
