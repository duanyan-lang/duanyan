//! SNARjs Hardware Flasher Abstraction
//!
//! Provides a unified `Flasher` trait for programming:
//! - FPGA boards (Gowin GW1NSR-4C via Gowin Programmer CLI)
//! - MCU targets (placeholder for future Espruino / OpenOCD integration)
//!
//! The concrete implementation shells out to vendor tooling so we never reimplement
//! EDA synthesis inside the compiler.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A single programming target description.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Target {
    pub kind: TargetKind,
    pub port: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TargetKind {
    FpgaGowin,
    McuStm32,
    McuEsp32,
    McuArduino,
    McuRpi,
}

impl TargetKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gowin" | "fpga" | "gw1nsr-4c" => Some(TargetKind::FpgaGowin),
            "stm32" | "stlink" => Some(TargetKind::McuStm32),
            "esp32" | "esp" => Some(TargetKind::McuEsp32),
            "arduino" | "arduino-avr" | "arduino-arm" | "uno" | "mega" | "due" => Some(TargetKind::McuArduino),
            "rpi" | "raspberry" | "raspberrypi" => Some(TargetKind::McuRpi),
            _ => None,
        }
    }
}

/// Programming result.
#[derive(Debug)]
pub struct FlashResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Common flasher interface.
pub trait Flasher {
    /// Program the device with the given firmware artifact.
    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String>;

    /// Human-readable name.
    fn name(&self) -> &str;
}

/// Firmware artifact produced by the compiler/packager.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FirmwareArtifact {
    pub hex_path: PathBuf,
    pub bin_path: Option<PathBuf>,
    pub verilog_path: Option<PathBuf>,
    pub entry_address: u32,
}

impl FirmwareArtifact {
    pub fn from_hex(hex_path: &Path) -> Result<Self, String> {
        if !hex_path.exists() {
            return Err(format!("Firmware hex not found: {}", hex_path.display()));
        }
        Ok(Self {
            hex_path: hex_path.to_path_buf(),
            bin_path: None,
            verilog_path: None,
            entry_address: 0,
        })
    }
}

/// Gowin FPGA flasher: invokes `programmer_cli` if available, otherwise
/// falls back to embedding the .hex into a Verilog ROM and printing instructions.
pub struct GowinFlasher {
    programmer_cli: Option<PathBuf>,
    cable: String,
}

impl GowinFlasher {
    pub fn new(programmer_cli: Option<PathBuf>, cable: &str) -> Self {
        Self {
            programmer_cli,
            cable: cable.to_string(),
        }
    }

    pub fn detect() -> Self {
        let candidates = [
            r"C:\Gowin\Gowin_V1.9.9.03_x64\IDE\bin\programmer_cli.exe",
            r"C:\Gowin\Gowin_V1.9.9\IDE\bin\programmer_cli.exe",
            r"C:\Program Files\Gowin\Gowin_V1.9.9.03_x64\IDE\bin\programmer_cli.exe",
        ];
        let programmer_cli = candidates.iter().map(PathBuf::from).find(|p| p.exists());
        Self::new(programmer_cli, "USB2")
    }
}

impl Flasher for GowinFlasher {
    fn name(&self) -> &str {
        "Gowin FPGA"
    }

    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String> {
        let hex_content = fs::read_to_string(&artifact.hex_path)
            .map_err(|e| format!("读取 hex 失败: {}", e))?;

        if let Some(cli) = &self.programmer_cli {
            let output = Command::new(cli)
                .args(&["--cable", &self.cable, "--device", "GW1NSR-4C", "--operation_index", "1"])
                .arg("--fs_file")
                .arg(&artifact.hex_path)
                .output()
                .map_err(|e| format!("启动 Gowin programmer_cli 失败: {}", e))?;

            Ok(FlashResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        } else {
            // Fallback: generate a Verilog ROM module and tell the user to run Gowin Programmer.
            let verilog = generate_gowin_rom(&hex_content, artifact.entry_address);
            let vl_path = artifact.hex_path.with_extension("rom.v");
            fs::write(&vl_path, verilog)
                .map_err(|e| format!("写入 {} 失败: {}", vl_path.display(), e))?;

            let msg = format!(
                "未找到 Gowin programmer_cli。\n已生成 ROM 文件: {}\n请用 Gowin Programmer 手动烧录该 Verilog ROM。",
                vl_path.display()
            );
            Ok(FlashResult {
                success: true,
                stdout: msg,
                stderr: String::new(),
            })
        }
    }
}

/// Mock flasher for CI / unit tests.
pub struct MockFlasher;

impl Flasher for MockFlasher {
    fn name(&self) -> &str {
        "Mock"
    }

    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String> {
        Ok(FlashResult {
            success: true,
            stdout: format!("[MOCK] 已烧录 {}", artifact.hex_path.display()),
            stderr: String::new(),
        })
    }
}

/// ESP32 flasher: invokes esptool.py or prints manual instructions.
pub struct Esp32Flasher {
    esptool: Option<PathBuf>,
    port: Option<String>,
}

impl Esp32Flasher {
    pub fn new(port: Option<String>) -> Self {
        let esptool = Self::detect_esptool();
        Self { esptool, port }
    }

    fn detect_esptool() -> Option<PathBuf> {
        // Try common esptool locations
        let candidates = [
            "esptool.py",
            "esptool",
            r"C:\Espressif\frameworks\esp-idf\tools\esptool_py\esptool.py",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() {
                return Some(p);
            }
        }
        // Try `python -m esptool` via `where` on Windows
        if cfg!(target_os = "windows") {
            if let Ok(output) = Command::new("where").arg("esptool.py").output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Some(PathBuf::from(path.lines().next().unwrap_or("")));
                    }
                }
            }
        }
        None
    }
}

impl Flasher for Esp32Flasher {
    fn name(&self) -> &str {
        "ESP32 (esptool)"
    }

    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String> {
        let port = self.port.as_deref().unwrap_or("COM3");
        let bin_path = artifact.bin_path.as_ref()
            .or(Some(&artifact.hex_path))
            .unwrap();

        if let Some(esptool) = &self.esptool {
            let output = Command::new(esptool)
                .args(&[
                    "--chip", "esp32",
                    "--port", port,
                    "write_flash",
                    "0x10000",
                ])
                .arg(bin_path)
                .output()
                .map_err(|e| format!("启动 esptool 失败: {}", e))?;

            Ok(FlashResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        } else {
            let msg = format!(
                "未找到 esptool.py。请手动烧录:\n\
                 1. 安装 ESP-IDF: https://docs.espressif.com/projects/esp-idf/\n\
                 2. cd <ESP-IDF项目目录>\n\
                 3. idf.py build\n\
                 4. idf.py -p {} flash monitor\n\
                 或: python -m esptool --chip esp32 --port {} write_flash 0x10000 {}",
                port, port, bin_path.display()
            );
            Ok(FlashResult {
                success: true,
                stdout: msg,
                stderr: String::new(),
            })
        }
    }
}

/// Arduino flasher: invokes arduino-cli or prints manual instructions.
pub struct ArduinoFlasher {
    arduino_cli: Option<PathBuf>,
    port: Option<String>,
    fqbn: String,
}

impl ArduinoFlasher {
    pub fn new(port: Option<String>, fqbn: Option<String>) -> Self {
        let arduino_cli = Self::detect_arduino_cli();
        Self {
            arduino_cli,
            port,
            fqbn: fqbn.unwrap_or_else(|| "arduino:avr:uno".to_string()),
        }
    }

    fn detect_arduino_cli() -> Option<PathBuf> {
        let candidates = [
            "arduino-cli",
            r"C:\Users\{}\AppData\Local\Arduino15\arduino-cli.exe",
            r"C:\Program Files\arduino-cli\arduino-cli.exe",
            "/usr/local/bin/arduino-cli",
            "/home/linuxbrew/.linuxbrew/bin/arduino-cli",
        ];
        for c in &candidates {
            let path_str = c.replace("{}", &std::env::var("USERNAME").unwrap_or_default());
            let p = PathBuf::from(&path_str);
            if p.exists() {
                return Some(p);
            }
        }
        // Windows: try `where`
        if cfg!(target_os = "windows") {
            if let Ok(output) = Command::new("where").arg("arduino-cli").output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Some(PathBuf::from(path.lines().next().unwrap_or("")));
                    }
                }
            }
        }
        None
    }
}

impl Flasher for ArduinoFlasher {
    fn name(&self) -> &str {
        "Arduino (arduino-cli)"
    }

    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String> {
        let port = self.port.as_deref().unwrap_or("COM3");

        if let Some(cli) = &self.arduino_cli {
            // 编译
            let sketch_dir = artifact.hex_path.parent()
                .unwrap_or_else(|| Path::new("."));
            let compile_out = Command::new(cli)
                .args(&["compile", "--fqbn", &self.fqbn])
                .arg(sketch_dir)
                .output()
                .map_err(|e| format!("启动 arduino-cli compile 失败: {}", e))?;

            if !compile_out.status.success() {
                return Ok(FlashResult {
                    success: false,
                    stdout: String::from_utf8_lossy(&compile_out.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&compile_out.stderr).to_string(),
                });
            }

            // 烧录
            let upload_out = Command::new(cli)
                .args(&["upload", "-p", port, "--fqbn", &self.fqbn])
                .arg(sketch_dir)
                .output()
                .map_err(|e| format!("启动 arduino-cli upload 失败: {}", e))?;

            Ok(FlashResult {
                success: upload_out.status.success(),
                stdout: format!(
                    "编译:\n{}\n烧录:\n{}",
                    String::from_utf8_lossy(&compile_out.stdout),
                    String::from_utf8_lossy(&upload_out.stdout)
                ),
                stderr: format!(
                    "{}{}",
                    String::from_utf8_lossy(&compile_out.stderr),
                    String::from_utf8_lossy(&upload_out.stderr)
                ),
            })
        } else {
            let msg = format!(
                "未找到 arduino-cli。请手动烧录:\n\
                 1. 安装 arduino-cli: https://arduino.github.io/arduino-cli/\n\
                 2. arduino-cli core install arduino:avr\n\
                 3. arduino-cli compile --fqbn {} <sketch_dir>\n\
                 4. arduino-cli upload -p {} --fqbn {} <sketch_dir>",
                self.fqbn, port, self.fqbn
            );
            Ok(FlashResult {
                success: true,
                stdout: msg,
                stderr: String::new(),
            })
        }
    }
}

/// STM32 flasher: invokes st-flash or openocd, or prints manual instructions.
pub struct Stm32Flasher {
    st_flash: Option<PathBuf>,
    openocd: Option<PathBuf>,
}

impl Stm32Flasher {
    pub fn new() -> Self {
        let st_flash = Self::detect_tool("st-flash");
        let openocd = Self::detect_tool("openocd");
        Self { st_flash, openocd }
    }

    fn detect_tool(name: &str) -> Option<PathBuf> {
        let candidates = [name, &format!("/usr/bin/{}", name), &format!("/usr/local/bin/{}", name)];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() { return Some(p); }
        }
        if cfg!(target_os = "windows") {
            if let Ok(output) = Command::new("where").arg(name).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Some(PathBuf::from(path.lines().next().unwrap_or("")));
                    }
                }
            }
        }
        None
    }
}

impl Flasher for Stm32Flasher {
    fn name(&self) -> &str {
        "STM32 (st-flash/openocd)"
    }

    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String> {
        let bin_path = artifact.bin_path.as_ref()
            .or(Some(&artifact.hex_path))
            .unwrap();

        if let Some(st_flash) = &self.st_flash {
            let output = Command::new(st_flash)
                .args(&["write"])
                .arg(bin_path)
                .arg("0x08000000")
                .output()
                .map_err(|e| format!("启动 st-flash 失败: {}", e))?;

            Ok(FlashResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        } else if let Some(openocd) = &self.openocd {
            let output = Command::new(openocd)
                .args(&[
                    "-f", "interface/stlink.cfg",
                    "-f", "target/stm32f1x.cfg",
                    "-c", &format!("program {} reset exit", bin_path.display()),
                ])
                .output()
                .map_err(|e| format!("启动 openocd 失败: {}", e))?;

            Ok(FlashResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        } else {
            let msg = format!(
                "未找到 st-flash 或 openocd。请手动烧录:\n\
                 1. 安装 stlink: https://github.com/stlink-org/stlink\n\
                 2. st-flash write {} 0x08000000\n\
                 或安装 openocd 并使用 openocd.cfg 配置",
                bin_path.display()
            );
            Ok(FlashResult { success: true, stdout: msg, stderr: String::new() })
        }
    }
}

/// RPi flasher: scp + ssh 部署到树莓派
pub struct RpiFlasher {
    host: String,
    user: String,
}

impl RpiFlasher {
    pub fn new(host: Option<String>, user: Option<String>) -> Self {
        Self {
            host: host.unwrap_or_else(|| "raspberrypi.local".to_string()),
            user: user.unwrap_or_else(|| "pi".to_string()),
        }
    }
}

impl Flasher for RpiFlasher {
    fn name(&self) -> &str {
        "Raspberry Pi (scp/ssh)"
    }

    fn flash(&self, artifact: &FirmwareArtifact) -> Result<FlashResult, String> {
        let bin_path = artifact.bin_path.as_ref()
            .or(Some(&artifact.hex_path))
            .unwrap();

        let msg = format!(
            "树莓派部署指引:\n\
             1. 编译: cd <项目目录> && make\n\
             2. 复制: scp {} {}@{}:/tmp/\n\
             3. 执行: ssh {}@{} \"sudo /tmp/{}\"\n\
             或使用自动脚本: bash run.sh {} {}",
            bin_path.display(), self.user, self.host,
            self.user, self.host, bin_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
            self.host, self.user
        );
        Ok(FlashResult { success: true, stdout: msg, stderr: String::new() })
    }
}

/// Factory: pick a flasher by target.
pub fn create_flasher(target: &Target) -> Box<dyn Flasher> {
    match target.kind {
        TargetKind::FpgaGowin => Box::new(GowinFlasher::detect()),
        TargetKind::McuEsp32 => Box::new(Esp32Flasher::new(target.port.clone())),
        TargetKind::McuStm32 => Box::new(Stm32Flasher::new()),
        TargetKind::McuArduino => Box::new(ArduinoFlasher::new(target.port.clone(), None)),
        TargetKind::McuRpi => Box::new(RpiFlasher::new(None, None)),
    }
}

/// Convert a SNARjs .hex file (one 32-bit hex value per line) into a simple
/// Gowin-compatible Verilog ROM module.
fn generate_gowin_rom(hex_content: &str, base_addr: u32) -> String {
    let words: Vec<String> = hex_content
        .lines()
        .map(|l| l.split("//").next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let v = u32::from_str_radix(l, 16).unwrap_or(0);
            format!("        32'h{:08X}", v)
        })
        .collect();

    let depth = words.len().max(1);
    let words_str = words.join(",\n");

    format!(
        "// Auto-generated by SNARjs for Gowin GW1NSR-4C\n\
         // Source hex base address: 0x{:08X}\n\
         module snarjs_rom (\n\
             input wire clk,\n\
             input wire [$clog2({DEPTH})-1:0] addr,\n\
             output reg [31:0] data\n\
         );\n\
             always @(posedge clk) begin\n\
                 case (addr)\n{WORDS}\n\
                     default: data = 32'h00000000;\n\
                 endcase\n\
             end\n\
         endmodule\n",
        base_addr,
        DEPTH = depth,
        WORDS = words_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gowin_rom_generation() {
        let hex = "10000000\n20000001\n00000000\n";
        let vl = generate_gowin_rom(hex, 0);
        assert!(vl.contains("module snarjs_rom"));
        assert!(vl.contains("32'h10000000"));
        assert!(vl.contains("32'h20000001"));
    }
}
