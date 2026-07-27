//! 树莓派项目生成器
//!
//! Phase E: 树莓派后端
//! 生成 Linux 原生 C 项目，使用 sysfs GPIO，可选 scp/ssh 远程部署

use std::fs;
use std::path::{Path, PathBuf};

use crate::c_codegen::CGenerateResult;

/// 树莓派项目路径
pub struct RpiProjectPaths {
    pub project_dir: PathBuf,
    pub main_c: PathBuf,
    pub makefile: PathBuf,
    pub run_sh: PathBuf,
}

/// 生成树莓派项目
pub fn generate_rpi_project(
    c_result: &CGenerateResult,
    base_dir: &Path,
    stem: &str,
) -> Result<RpiProjectPaths, String> {
    let project_name = format!("{}_rpi", sanitize_name(stem));
    let project_dir = base_dir.join(&project_name);
    fs::create_dir_all(&project_dir)
        .map_err(|e| format!("创建 RPi 项目目录失败: {}", e))?;

    // main.c
    let main_c = project_dir.join("main.c");
    fs::write(&main_c, &c_result.source)
        .map_err(|e| format!("写入 main.c 失败: {}", e))?;

    // Makefile
    let makefile = project_dir.join("Makefile");
    fs::write(&makefile, generate_makefile(stem))
        .map_err(|e| format!("写入 Makefile 失败: {}", e))?;

    // run.sh (scp + ssh 部署脚本)
    let run_sh = project_dir.join("run.sh");
    fs::write(&run_sh, generate_run_script(stem))
        .map_err(|e| format!("写入 run.sh 失败: {}", e))?;

    Ok(RpiProjectPaths {
        project_dir,
        main_c,
        makefile,
        run_sh,
    })
}

fn generate_makefile(stem: &str) -> String {
    let name = sanitize_name(stem);
    format!(
r#"# SNARjs Raspberry Pi Makefile (auto-generated)
CC      = gcc
TARGET  = {name}

CFLAGS  = -std=c99 -Wall -O2
LDFLAGS =

# 交叉编译 (可选)
# CC    = arm-linux-gnueabihf-gcc

SRCS    = main.c
OBJS    = $(SRCS:.c=.o)

all: $(TARGET)

$(TARGET): $(OBJS)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJS) $(TARGET)

run: $(TARGET)
	sudo ./$(TARGET)

.PHONY: all clean run
"#,
        name = name
    )
}

fn generate_run_script(stem: &str) -> String {
    let name = sanitize_name(stem);
    format!(
r#"#!/bin/bash
# SNARjs Raspberry Pi 部署脚本 (auto-generated)
# 用法: ./run.sh [pi_host] [pi_user]
#
# 示例: ./run.sh 192.168.1.100 pi

PI_HOST=${{1:-raspberrypi.local}}
PI_USER=${{2:-pi}}
TARGET={name}

echo "编译本地..."
make clean && make

echo "复制到树莓派 $PI_USER@$PI_HOST ..."
scp $TARGET $PI_USER@$PI_HOST:/tmp/$TARGET

echo "远程执行..."
ssh $PI_USER@$PI_HOST "sudo /tmp/$TARGET"

echo "清理远程文件..."
ssh $PI_USER@$PI_HOST "rm /tmp/$TARGET"

echo "完成!"
"#,
        name = name
    )
}

fn sanitize_name(name: &str) -> String {
    let s: String = name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() { "snar_app".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("my-app"), "my_app");
        assert_eq!(sanitize_name(""), "snar_app");
    }

    #[test]
    fn test_makefile_contains_gcc() {
        let mk = generate_makefile("blink");
        assert!(mk.contains("CC      = gcc"));
        assert!(mk.contains("TARGET  = blink"));
        assert!(mk.contains("sudo"));
    }

    #[test]
    fn test_run_script() {
        let sh = generate_run_script("blink");
        assert!(sh.contains("scp"));
        assert!(sh.contains("ssh"));
        assert!(sh.contains("TARGET=blink"));
    }
}
