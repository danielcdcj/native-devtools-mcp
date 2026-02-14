use adb_client::server::ADBServer;
use adb_client::server_device::ADBServerDevice;
use adb_client::ADBDeviceExt;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AndroidDeviceInfo {
    pub serial: String,
    pub state: String,
}

pub struct AndroidDevice {
    pub serial: String,
    device: ADBServerDevice,
}

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

    pub fn shell(&mut self, command: &str) -> Result<String, String> {
        let mut output = Vec::new();
        self.device
            .shell_command(&command, Some(&mut output), None)
            .map_err(|e| format!("Shell command failed: {}", e))?;

        String::from_utf8(output).map_err(|e| format!("Shell output is not valid UTF-8: {}", e))
    }

    /// Run a shell command with arguments joined by spaces.
    pub fn shell_args(&mut self, args: &[&str]) -> Result<String, String> {
        self.shell(&args.join(" "))
    }

    /// Run a shell command and capture raw bytes output.
    pub fn shell_bytes(&mut self, args: &[&str], output: &mut Vec<u8>) -> Result<(), String> {
        let command = args.join(" ");
        self.device
            .shell_command(&command, Some(output), None)
            .map_err(|e| format!("Shell command failed: {}", e))?;
        Ok(())
    }

    pub fn framebuffer_png(&mut self) -> Result<Vec<u8>, String> {
        self.device
            .framebuffer_bytes()
            .map_err(|e| format!("Failed to capture framebuffer: {}", e))
    }
}
