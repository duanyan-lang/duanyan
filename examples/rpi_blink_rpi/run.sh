#!/bin/bash
# DUANYAN Raspberry Pi 部署脚本 (auto-generated)
# 用法: ./run.sh [pi_host] [pi_user]
#
# 示例: ./run.sh 192.168.1.100 pi

PI_HOST=${1:-raspberrypi.local}
PI_USER=${2:-pi}
TARGET=rpi_blink

echo "编译本地..."
make clean && make

echo "复制到树莓派 $PI_USER@$PI_HOST ..."
scp $TARGET $PI_USER@$PI_HOST:/tmp/$TARGET

echo "远程执行..."
ssh $PI_USER@$PI_HOST "sudo /tmp/$TARGET"

echo "清理远程文件..."
ssh $PI_USER@$PI_HOST "rm /tmp/$TARGET"

echo "完成!"
