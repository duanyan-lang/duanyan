//! 硬件 API C helper 生成（任务 B3 从 c_codegen/mod.rs 拆出，纯搬运）
use super::*;

impl CCodeGenerator {
    /// 生成硬件 API 辅助函数
    pub(super) fn generate_hw_helpers(&mut self) {
        match self.target {
            CTarget::Esp32S3Bare => {
                // 裸机 HAL 已在头部整体注入, 无需额外 helper
                // v1.6 P3: --target duanyan-spine 的 Mode-B 应答端骨架 (走裸机 UART0 FIFO 直发)
                if self.hw_imports.iter().any(|s| s.starts_with("modeb:")) {
                    self.output.push_str("/* DUANYAN Mode-B spine skeleton (v1.6 P3, --target duanyan-spine) — 组帧端骨架;\n");
                    self.output.push_str("   ⚠ 只走 UART0 FIFO 直发, 不替换真机 uart_linkm6 位啮时序/应答端 */\n");
                    self.output.push_str("static inline uint8_t _duanyan_modeb_crc8(const uint8_t *d, int n) {\n");
                    self.output.push_str("    uint8_t crc = 0;\n");
                    self.output.push_str("    for (int i = 0; i < n; i++) {\n");
                    self.output.push_str("        crc ^= d[i];\n");
                    self.output.push_str("        for (int b = 0; b < 8; b++) crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x07) : (uint8_t)(crc << 1);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    return crc;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_send(uint8_t cmd, const uint8_t *pl, uint8_t len) {\n");
                    self.output.push_str("    uint8_t buf[5 + 64];\n");
                    self.output.push_str("    if (len > 64) len = 64;\n");
                    self.output.push_str("    buf[0] = 0xAA; buf[1] = 0x55; buf[2] = len; buf[3] = cmd;\n");
                    self.output.push_str("    for (int i = 0; i < len; i++) buf[4 + i] = pl[i];\n");
                    self.output.push_str("    buf[4 + len] = _duanyan_modeb_crc8(&buf[2], 2 + len);\n");
                    self.output.push_str("    for (int i = 0; i < 8; i++) duanyan_uart_putc('U');   /* 位啮自校准突发 */\n");
                    self.output.push_str("    for (int i = 0; i < 5 + len; i++) duanyan_uart_putc((char)buf[i]);\n");
                    self.output.push_str("    return 5 + len;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_joint_set(uint8_t joint, uint16_t ang, uint16_t lo, uint16_t hi) {\n");
                    self.output.push_str("    uint8_t pl[7] = { joint, (uint8_t)(ang>>8), (uint8_t)(ang&0xFF), (uint8_t)(lo>>8), (uint8_t)(lo&0xFF), (uint8_t)(hi>>8), (uint8_t)(hi&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x20, pl, 7);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_home(void) {\n");
                    self.output.push_str("    uint8_t pl[13] = { 4, 0,0x03,0x84, 1,0x03,0x84, 2,0x03,0x84, 3,0x03,0x84 };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x22, pl, 13);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_sync(const uint8_t *items, uint8_t n) {\n");
                    self.output.push_str("    uint8_t pl[1 + 8*5];\n");
                    self.output.push_str("    if (n == 0 || n > 8) return 0;\n");
                    self.output.push_str("    pl[0] = n;\n");
                    self.output.push_str("    for (int i = 0; i < n*5; i++) pl[1+i] = items[i];\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x30, pl, 1 + n*5);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_heartbeat(uint32_t uptime) {\n");
                    self.output.push_str("    uint8_t pl[4] = { (uint8_t)(uptime&0xFF), (uint8_t)((uptime>>8)&0xFF), (uint8_t)((uptime>>16)&0xFF), (uint8_t)((uptime>>24)&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x01, pl, 4);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::Esp32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers */\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_INPUT);\n");
                    self.output.push_str("    gpio_set_pull_mode(pin, GPIO_PULLUP_ONLY);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) { gpio_set_level(pin, 1); }\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin)  { gpio_set_level(pin, 0); }\n");
                    self.output.push_str("static inline int  duanyan_gpio_read(int pin) { return gpio_get_level(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) {\n");
                    self.output.push_str("    uart_config_t cfg = {\n");
                    self.output.push_str("        .baud_rate = baud,\n");
                    self.output.push_str("        .data_bits = UART_DATA_8_BITS,\n");
                    self.output.push_str("        .parity    = UART_PARITY_DISABLE,\n");
                    self.output.push_str("        .stop_bits = UART_STOP_BITS_1,\n");
                    self.output.push_str("        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    uart_param_config(UART_NUM_0, &cfg);\n");
                    self.output.push_str("    uart_driver_install(UART_NUM_0, 1024, 0, 0, NULL, 0);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) {\n");
                    self.output.push_str("    uart_write_bytes(UART_NUM_0, str, strlen(str));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM (LEDC) helpers */\n");
                    self.output.push_str("static int _duanyan_pwm_channel = 0;\n");
                    self.output.push_str("static inline int duanyan_pwm_output(int pin, int freq) {\n");
                    self.output.push_str("    int ch = _duanyan_pwm_channel++;\n");
                    self.output.push_str("    ledc_timer_config_t tcfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .duty_resolution = LEDC_TIMER_8_BIT,\n");
                    self.output.push_str("        .timer_num = LEDC_TIMER_0,\n");
                    self.output.push_str("        .freq_hz = freq,\n");
                    self.output.push_str("        .clk_cfg = LEDC_AUTO_CLK,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_timer_config(&tcfg);\n");
                    self.output.push_str("    ledc_channel_config_t ccfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .channel = ch,\n");
                    self.output.push_str("        .timer_sel = LEDC_TIMER_0,\n");
                    self.output.push_str("        .intr_type = LEDC_INTR_DISABLE,\n");
                    self.output.push_str("        .gpio_num = pin,\n");
                    self.output.push_str("        .duty = 0,\n");
                    self.output.push_str("        .hpoint = 0,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_channel_config(&ccfg);\n");
                    self.output.push_str("    return ch;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int channel, int duty) {\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, channel, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers */\n");
                    self.output.push_str("static inline void duanyan_adc_init(int channel) {\n");
                    self.output.push_str("    adc1_config_width(ADC_WIDTH_BIT_12);\n");
                    self.output.push_str("    adc1_config_channel_atten(channel, ADC_ATTEN_DB_11);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) {\n");
                    self.output.push_str("    return adc1_get_raw(channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_wifi {
                    self.output.push_str("/* DUANYAN WiFi helper */\n");
                    self.output.push_str("static void duanyan_wifi_connect(const char* ssid, const char* pass) {\n");
                    self.output.push_str("    nvs_flash_init();\n");
                    self.output.push_str("    esp_netif_init();\n");
                    self.output.push_str("    esp_event_loop_create_default();\n");
                    self.output.push_str("    esp_netif_create_default_wifi_sta();\n");
                    self.output.push_str("    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();\n");
                    self.output.push_str("    esp_wifi_init(&cfg);\n");
                    self.output.push_str("    wifi_config_t wcfg = { .sta = { .threshold.authmode = WIFI_AUTH_WPA2_PSK } };\n");
                    self.output.push_str("    strncpy((char*)wcfg.sta.ssid, ssid, sizeof(wcfg.sta.ssid)-1);\n");
                    self.output.push_str("    strncpy((char*)wcfg.sta.password, pass, sizeof(wcfg.sta.password)-1);\n");
                    self.output.push_str("    esp_wifi_set_mode(WIFI_MODE_STA);\n");
                    self.output.push_str("    esp_wifi_set_config(WIFI_IF_STA, &wcfg);\n");
                    self.output.push_str("    esp_wifi_start();\n");
                    self.output.push_str("    esp_wifi_connect();\n");
                    self.output.push_str("}\n\n");
                }
                if self.needs_chip_info {
                    self.output.push_str("/* DUANYAN Chip Info helpers */\n");
                    self.output.push_str("static const char* duanyan_chip_model_name(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    switch (info.model) {\n");
                    self.output.push_str("        case CHIP_ESP32:   return \"ESP32\";\n");
                    self.output.push_str("        case CHIP_ESP32S2: return \"ESP32-S2\";\n");
                    self.output.push_str("        case CHIP_ESP32S3: return \"ESP32-S3\";\n");
                    self.output.push_str("        case CHIP_ESP32C3: return \"ESP32-C3\";\n");
                    self.output.push_str("        case CHIP_ESP32C6: return \"ESP32-C6\";\n");
                    self.output.push_str("        case CHIP_ESP32H2: return \"ESP32-H2\";\n");
                    self.output.push_str("        default:           return \"Unknown\";\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_cores(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    return info.cores;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_revision(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    return info.revision;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_flash_size(void) {\n");
                    self.output.push_str("    uint32_t size = 0;\n");
                    self.output.push_str("    esp_flash_get_size(NULL, &size);\n");
                    self.output.push_str("    return (int)(size / (1024 * 1024));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_free_heap(void) {\n");
                    self.output.push_str("    return (int)esp_get_free_heap_size();\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_cpu_freq(void) {\n");
                    self.output.push_str("    return CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint32_t duanyan_chip_id(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    return ((uint32_t)mac[2] << 24) | ((uint32_t)mac[3] << 16) | ((uint32_t)mac[4] << 8) | mac[5];\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static const char* duanyan_chip_sdk_version(void) {\n");
                    self.output.push_str("    return esp_get_idf_version();\n");
                    self.output.push_str("}\n");
                    // 设备唯一 ID: MAC → SHA-256 → "DUANYAN-XXXX-XXXX-XXXX"
                    // 缓冲 28 = 前缀 12 + 12 hex + 2 连字符 + NUL（旧 SNAR- 时代为 20）
                    self.output.push_str("static char _duanyan_device_id[28];\n");
                    self.output.push_str("static const char* duanyan_chip_device_id(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    uint8_t hash[32];\n");
                    self.output.push_str("    mbedtls_sha256(mac, 6, hash, 0);\n");
                    self.output.push_str("    snprintf(_duanyan_device_id, sizeof(_duanyan_device_id),\n");
                    self.output.push_str("        \"DUANYAN-%02X%02X-%02X%02X-%02X%02X\",\n");
                    self.output.push_str("        hash[0], hash[1], hash[2], hash[3], hash[4], hash[5]);\n");
                    self.output.push_str("    return _duanyan_device_id;\n");
                    self.output.push_str("}\n");
                    // MAC 地址字符串
                    self.output.push_str("static char _duanyan_mac_str[18];\n");
                    self.output.push_str("static const char* duanyan_chip_mac(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    snprintf(_duanyan_mac_str, sizeof(_duanyan_mac_str),\n");
                    self.output.push_str("        \"%02X:%02X:%02X:%02X:%02X:%02X\",\n");
                    self.output.push_str("        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);\n");
                    self.output.push_str("    return _duanyan_mac_str;\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_ai {
                    self.output.push_str("/* DUANYAN 锻言Mini AI helpers */\n");
                    self.output.push_str("static duanyan_brain_t _duanyan_brain;\n");
                    self.output.push_str("static inline int duanyan_brain_init(const char* model_path) {\n");
                    self.output.push_str("    return duanyan_ai_load_model(&_duanyan_brain, model_path);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_brain_classify(float* input, int input_len,\n");
                    self.output.push_str("                                       const char** labels, int n_labels) {\n");
                    self.output.push_str("    return duanyan_ai_classify(&_duanyan_brain, input, input_len, labels, n_labels);\n");
                    self.output.push_str("}\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (ESP32 LEDC)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (ESP32 LEDC 50Hz PWM) */\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 16\n");
                    self.output.push_str("static int _duanyan_servo_channel = 0;\n");
                    self.output.push_str("static int _duanyan_servo_min[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_max[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_pin[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int ch = _duanyan_servo_channel++;\n");
                    self.output.push_str("    _duanyan_servo_min[ch] = min_us;\n");
                    self.output.push_str("    _duanyan_servo_max[ch] = max_us;\n");
                    self.output.push_str("    _duanyan_servo_pin[ch] = pin;\n");
                    self.output.push_str("    _duanyan_servo_count = ch + 1;\n");
                    self.output.push_str("    ledc_timer_config_t tcfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .duty_resolution = LEDC_TIMER_16_BIT,\n");
                    self.output.push_str("        .timer_num = LEDC_TIMER_1,\n");
                    self.output.push_str("        .freq_hz = 50,\n");
                    self.output.push_str("        .clk_cfg = LEDC_AUTO_CLK,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_timer_config(&tcfg);\n");
                    self.output.push_str("    ledc_channel_config_t ccfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .channel = ch,\n");
                    self.output.push_str("        .timer_sel = LEDC_TIMER_1,\n");
                    self.output.push_str("        .intr_type = LEDC_INTR_DISABLE,\n");
                    self.output.push_str("        .gpio_num = pin,\n");
                    self.output.push_str("        .duty = 0,\n");
                    self.output.push_str("        .hpoint = 0,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_channel_config(&ccfg);\n");
                    self.output.push_str("    return ch;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    int ch = -1;\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pin[i] == pin) { ch = i; break; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (ch < 0) return;\n");
                    self.output.push_str("    if (angle < 0) angle = 0;\n");
                    self.output.push_str("    if (angle > 180) angle = 180;\n");
                    self.output.push_str("    int pulse_us = _duanyan_servo_min[ch] + (angle * (_duanyan_servo_max[ch] - _duanyan_servo_min[ch])) / 180;\n");
                    self.output.push_str("    /* 16-bit duty: 65535 = 20000us period, pulse_us maps to duty */\n");
                    self.output.push_str("    uint32_t duty = (uint32_t)((uint64_t)pulse_us * 65535 / 20000);\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, ch, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, ch);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (ESP32 LEDC + GPIO) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_ch[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    gpio_set_direction(in1, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("    gpio_set_direction(in2, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("    int ch = _duanyan_servo_channel++;\n");
                    self.output.push_str("    _duanyan_motor_ch[id] = ch;\n");
                    self.output.push_str("    ledc_timer_config_t tcfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .duty_resolution = LEDC_TIMER_8_BIT,\n");
                    self.output.push_str("        .timer_num = LEDC_TIMER_2,\n");
                    self.output.push_str("        .freq_hz = 1000,\n");
                    self.output.push_str("        .clk_cfg = LEDC_AUTO_CLK,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_timer_config(&tcfg);\n");
                    self.output.push_str("    ledc_channel_config_t ccfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .channel = ch,\n");
                    self.output.push_str("        .timer_sel = LEDC_TIMER_2,\n");
                    self.output.push_str("        .intr_type = LEDC_INTR_DISABLE,\n");
                    self.output.push_str("        .gpio_num = ena,\n");
                    self.output.push_str("        .duty = 0,\n");
                    self.output.push_str("        .hpoint = 0,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_channel_config(&ccfg);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in1[id], 1);\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in2[id], 0);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in1[id], 0);\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in2[id], 1);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id], speed);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id]);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    gpio_set_level(_duanyan_motor_in1[id], 0);\n");
                    self.output.push_str("    gpio_set_level(_duanyan_motor_in2[id], 0);\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id], 0);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id]);\n");
                    self.output.push_str("}\n\n");
                }
                // v1.6 P0: duanyan:servo 关节层 helpers (PCA9685 I²C + ZX361 UART)
                // 语义对齐 DUANYAN joint_api: deg 先钳包络→线性映射 500..2500→按后端路由
                // (Esp32S3Bare 寄存器级目标的关节驱动走 mode-B 帧, 属 P3 duanyan-spine)
                if self.hw_imports.iter().any(|s| s.starts_with("servo:") || s.starts_with("robot:")) {
                    self.output.push_str("/* DUANYAN Joint/Servo helpers (v1.6 P0: PCA9685 I2C + ZX361 UART bus) */\n");
                    self.output.push_str("#define DUANYAN_JOINT_MAX 16\n");
                    self.output.push_str("#define DUANYAN_SERVO_US_MIN 500\n");
                    self.output.push_str("#define DUANYAN_SERVO_US_MAX 2500\n");
                    self.output.push_str("#ifndef DUANYAN_PCA9685_ADDR\n#define DUANYAN_PCA9685_ADDR 0x40\n#endif\n");
                    self.output.push_str("#ifndef DUANYAN_ZX361_UART\n#define DUANYAN_ZX361_UART UART_NUM_1\n#endif\n\n");
                    self.output.push_str("/* 关节绑定表: 后端(0=pca9685,1=zx361) + 通道/ID + 包络 */\n");
                    self.output.push_str("static uint8_t _hj_backend[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _hj_chan_id[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static float   _hj_min[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static float   _hj_max[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static float   _hj_home[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _hj_bound[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _duanyan_pca_addr = DUANYAN_PCA9685_ADDR;\n\n");
                    self.output.push_str("/* --- PCA9685 (16-ch PWM over I2C) --- */\n");
                    self.output.push_str("static void duanyan_pca9685_write_reg(uint8_t reg, uint8_t val) {\n");
                    self.output.push_str("    uint8_t buf[2] = { reg, val };\n");
                    self.output.push_str("    i2c_master_write_to_device(I2C_NUM_0, _duanyan_pca_addr, buf, sizeof(buf), 100 / portTICK_PERIOD_MS);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_pca9685_init(uint8_t addr) {\n");
                    self.output.push_str("    _duanyan_pca_addr = addr;\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0x00, 0x10);   /* MODE1: SLEEP */\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0xFE, 121);    /* PRE_SCALE: 25MHz/4096/50Hz-1 */\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0x00, 0x80);   /* MODE1: RESTART, wake */\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0x00, 0xA0);   /* MODE1: auto-increment */\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_pca9685_write_us(uint8_t channel, uint32_t us) {\n");
                    self.output.push_str("    if (us > DUANYAN_SERVO_US_MAX) us = DUANYAN_SERVO_US_MAX;\n");
                    self.output.push_str("    uint16_t off = (uint16_t)(us * 4096 / 20000);  /* 50Hz: 20000us period, 12-bit */\n");
                    self.output.push_str("    uint8_t base = 0x06 + 4 * channel;\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 0, 0);\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 1, 0);\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 2, (uint8_t)(off & 0xFF));\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 3, (uint8_t)((off >> 8) & 0x0F));\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("/* --- ZX361 bus servo (ASCII over UART) --- */\n");
                    self.output.push_str("static inline void duanyan_zx361_move(uint8_t id, uint16_t pwm, uint16_t ms) {\n");
                    self.output.push_str("    if (pwm < DUANYAN_SERVO_US_MIN) pwm = DUANYAN_SERVO_US_MIN;\n");
                    self.output.push_str("    if (pwm > DUANYAN_SERVO_US_MAX) pwm = DUANYAN_SERVO_US_MAX;\n");
                    self.output.push_str("    char buf[20];\n");
                    self.output.push_str("    int n = snprintf(buf, sizeof(buf), \"#%03uP%04uT%04u!\", id, pwm, ms);\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_ZX361_UART, buf, n);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_zx361_read_pos(uint8_t id) {\n");
                    self.output.push_str("    char buf[12];\n");
                    self.output.push_str("    int n = snprintf(buf, sizeof(buf), \"#%03uPRAD!\", id);\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_ZX361_UART, buf, n);\n");
                    self.output.push_str("    uint8_t rx[16] = {0};\n");
                    self.output.push_str("    int len = uart_read_bytes(DUANYAN_ZX361_UART, rx, sizeof(rx) - 1, 50 / portTICK_PERIOD_MS);\n");
                    self.output.push_str("    if (len < 10) return -1;\n");
                    self.output.push_str("    rx[len] = 0;\n");
                    self.output.push_str("    return atoi((char*)&rx[5]);   /* #IDP<pwm>! */\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_zx361_read_temp(uint8_t id) {\n");
                    self.output.push_str("    char buf[12];\n");
                    self.output.push_str("    int n = snprintf(buf, sizeof(buf), \"#%03uPRTE!\", id);\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_ZX361_UART, buf, n);\n");
                    self.output.push_str("    uint8_t rx[24] = {0};\n");
                    self.output.push_str("    int len = uart_read_bytes(DUANYAN_ZX361_UART, rx, sizeof(rx) - 1, 50 / portTICK_PERIOD_MS);\n");
                    self.output.push_str("    if (len < 15) return -1;\n");
                    self.output.push_str("    rx[len] = 0;\n");
                    self.output.push_str("    char ads[5] = { (char)rx[5], (char)rx[6], (char)rx[7], (char)rx[8], 0 };\n");
                    self.output.push_str("    float r = 10.0f * (float)atoi(ads) / 2048.0f;\n");
                    self.output.push_str("    return (int)(0.633916f * r * r - 13.2675f * r + 94.28351f);  /* 手册 V3.1 温度公式 */\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("/* --- Joint layer: bind + envelope-clamped write + read + home --- */\n");
                    self.output.push_str("static inline int duanyan_joint_bind(uint8_t joint, uint8_t backend, uint8_t chan_id,\n");
                    self.output.push_str("                                   float min_deg, float max_deg, float home_deg) {\n");
                    self.output.push_str("    if (joint >= DUANYAN_JOINT_MAX || min_deg >= max_deg) return 0;  /* 越界/倒置包络拒绑 */\n");
                    self.output.push_str("    _hj_backend[joint] = backend;\n");
                    self.output.push_str("    _hj_chan_id[joint] = chan_id;\n");
                    self.output.push_str("    _hj_min[joint] = min_deg;\n");
                    self.output.push_str("    _hj_max[joint] = max_deg;\n");
                    self.output.push_str("    _hj_home[joint] = home_deg;\n");
                    self.output.push_str("    _hj_bound[joint] = 1;\n");
                    self.output.push_str("    return 1;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_joint_write(uint8_t joint, float deg, uint16_t ms) {\n");
                    self.output.push_str("    if (joint >= DUANYAN_JOINT_MAX || !_hj_bound[joint]) return;  /* 未绑定跳过 */\n");
                    self.output.push_str("    if (deg < _hj_min[joint]) deg = _hj_min[joint];   /* 运行期包络钳位 */\n");
                    self.output.push_str("    if (deg > _hj_max[joint]) deg = _hj_max[joint];\n");
                    self.output.push_str("    float f = (deg - _hj_min[joint]) / (_hj_max[joint] - _hj_min[joint]);\n");
                    self.output.push_str("    uint16_t pwm = (uint16_t)(DUANYAN_SERVO_US_MIN + f * (DUANYAN_SERVO_US_MAX - DUANYAN_SERVO_US_MIN) + 0.5f);\n");
                    self.output.push_str("    if (_hj_backend[joint] == 1) duanyan_zx361_move(_hj_chan_id[joint], pwm, ms);\n");
                    self.output.push_str("    else duanyan_pca9685_write_us(_hj_chan_id[joint], pwm);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_joint_read(uint8_t joint) {\n");
                    self.output.push_str("    if (joint >= DUANYAN_JOINT_MAX || !_hj_bound[joint]) return -1;\n");
                    self.output.push_str("    if (_hj_backend[joint] == 1) return duanyan_zx361_read_pos(_hj_chan_id[joint]);\n");
                    self.output.push_str("    return -1;  /* PWM 后端无位置回读 (hw 脉冲真值走 mode-B 0x21) */\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_joint_home(void) {\n");
                    self.output.push_str("    for (uint8_t j = 0; j < DUANYAN_JOINT_MAX; j++)\n");
                    self.output.push_str("        if (_hj_bound[j]) duanyan_joint_write(j, _hj_home[j], 500);\n");
                    self.output.push_str("}\n\n");
                    // v1.6 P1: 执行器安全护栏 (对齐 DUANYAN safety_policy: temp>stall>load + 迟滞)
                    self.output.push_str("/* DUANYAN actuator safety guard (v1.6 P1: load/temp/stall, 对齐 DUANYAN) */\n");
                    self.output.push_str("#ifndef DUANYAN_LOAD_MAX\n#define DUANYAN_LOAD_MAX 900\n#endif\n");
                    self.output.push_str("#ifndef DUANYAN_TEMP_MAX\n#define DUANYAN_TEMP_MAX 65.0f\n#endif\n");
                    self.output.push_str("#ifndef DUANYAN_STALL_MS\n#define DUANYAN_STALL_MS 3000\n#endif\n");
                    self.output.push_str("#define DUANYAN_NORMAL_RECOVER 3\n");
                    self.output.push_str("/* 动作码: 0=None 1=TorqueOff 2=Stop 3=AlarmLed 4=EscalateHost */\n");
                    self.output.push_str("static uint16_t _duanyan_load_max = DUANYAN_LOAD_MAX;\n");
                    self.output.push_str("static float    _duanyan_temp_max = DUANYAN_TEMP_MAX;\n");
                    self.output.push_str("static uint32_t _duanyan_stall_ms = DUANYAN_STALL_MS;\n");
                    self.output.push_str("static uint8_t _hs_faulted[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _hs_normal_run[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static inline void duanyan_safety_limits(uint16_t load_max, float temp_max_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    _duanyan_load_max = load_max; _duanyan_temp_max = temp_max_c; _duanyan_stall_ms = stall_ms;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 无状态优先级判定: temp > stall > load (stall==0||0xFFFFFFFF 视为无堵转证据) */\n");
                    self.output.push_str("static inline int _duanyan_safety_eval(uint16_t load, float temp_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    if (temp_c > _duanyan_temp_max) return 1;\n");
                    self.output.push_str("    if (stall_ms != 0 && stall_ms != 0xFFFFFFFFu && stall_ms > _duanyan_stall_ms) return 2;\n");
                    self.output.push_str("    if (load > _duanyan_load_max) return 2;\n");
                    self.output.push_str("    return 0;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 无状态一次性判定 (对应 Safety.check) */\n");
                    self.output.push_str("static inline int _duanyan_safety_check(uint16_t load, float temp_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    return _duanyan_safety_eval(load, temp_c, stall_ms);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 带迟滞的按 id 状态化判定: 恢复需连续 DUANYAN_NORMAL_RECOVER 次正常 */\n");
                    self.output.push_str("static inline int _duanyan_safety_check_id(uint8_t id, uint16_t load, float temp_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    int act = _duanyan_safety_eval(load, temp_c, stall_ms);\n");
                    self.output.push_str("    if (id >= DUANYAN_JOINT_MAX) return act;\n");
                    self.output.push_str("    if (act == 0) {\n");
                    self.output.push_str("        if (_hs_faulted[id]) {\n");
                    self.output.push_str("            if (_hs_normal_run[id] < 255) _hs_normal_run[id]++;\n");
                    self.output.push_str("            if (_hs_normal_run[id] >= DUANYAN_NORMAL_RECOVER) { _hs_faulted[id] = 0; _hs_normal_run[id] = 0; return 0; }\n");
                    self.output.push_str("            return 4;\n");
                    self.output.push_str("        }\n");
                    self.output.push_str("        _hs_normal_run[id] = 0; return 0;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    _hs_faulted[id] = 1; _hs_normal_run[id] = 0; return act;\n");
                    self.output.push_str("}\n\n");
                }
                // v1.6 P2: duanyan:robot 轨迹数学 (下沉自 robotics.rs 纯数学, 与 robot_motion.rs 一致)
                if self.hw_imports.iter().any(|s| s.starts_with("robot:")) {
                    self.output.push_str("/* DUANYAN robot motion helpers (v1.6 P2: quintic/trapezoid + joint glide) */\n");
                    self.output.push_str("typedef struct { float c[6]; float T; } _duanyan_quintic_t;\n");
                    self.output.push_str("typedef struct { float pos; float vel; float acc; } _duanyan_traj_point_t;\n");
                    self.output.push_str("static inline _duanyan_quintic_t _duanyan_quintic_plan(float q0, float qf, float T) {\n");
                    self.output.push_str("    _duanyan_quintic_t h; float T2 = T*T, T3 = T2*T, T4 = T3*T, T5 = T4*T; float dq = qf - q0;\n");
                    self.output.push_str("    h.c[0] = q0; h.c[1] = 0.0f; h.c[2] = 0.0f;\n");
                    self.output.push_str("    h.c[3] = 10.0f*dq/T3; h.c[4] = -15.0f*dq/T4; h.c[5] = 6.0f*dq/T5; h.T = T;\n");
                    self.output.push_str("    return h;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline _duanyan_traj_point_t _duanyan_quintic_sample(_duanyan_quintic_t h, float t) {\n");
                    self.output.push_str("    _duanyan_traj_point_t p; float t2=t*t, t3=t2*t, t4=t3*t, t5=t4*t;\n");
                    self.output.push_str("    p.pos = h.c[0]+h.c[1]*t+h.c[2]*t2+h.c[3]*t3+h.c[4]*t4+h.c[5]*t5;\n");
                    self.output.push_str("    p.vel = h.c[1]+2*h.c[2]*t+3*h.c[3]*t2+4*h.c[4]*t3+5*h.c[5]*t4;\n");
                    self.output.push_str("    p.acc = 2*h.c[2]+6*h.c[3]*t+12*h.c[4]*t2+20*h.c[5]*t3;\n");
                    self.output.push_str("    return p;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("typedef struct { float t_accel, t_const, t_decel, v_max, accel, distance; } _duanyan_trapezoid_t;\n");
                    self.output.push_str("static inline _duanyan_trapezoid_t _duanyan_trapezoid_plan(float distance, float v_max, float a_max) {\n");
                    self.output.push_str("    _duanyan_trapezoid_t p; float t_accel = v_max/a_max; float d_accel = 0.5f*a_max*t_accel*t_accel;\n");
                    self.output.push_str("    if (2.0f*d_accel >= distance) {\n");
                    self.output.push_str("        float t = sqrtf(distance/a_max);\n");
                    self.output.push_str("        p.t_accel=t; p.t_const=0.0f; p.t_decel=t; p.v_max=a_max*t; p.accel=a_max; p.distance=distance;\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        float d_const = distance - 2.0f*d_accel;\n");
                    self.output.push_str("        p.t_accel=t_accel; p.t_const=d_const/v_max; p.t_decel=t_accel; p.v_max=v_max; p.accel=a_max; p.distance=distance;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    return p;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 五次采样 steps 点平滑移动关节 idx: from→to, 总时长 T 秒 (呼应真机 0x23 track glide) */\n");
                    self.output.push_str("static inline void _duanyan_joint_glide(uint8_t idx, float from, float to, float T, int steps) {\n");
                    self.output.push_str("    if (steps < 2) steps = 2;\n");
                    self.output.push_str("    _duanyan_quintic_t h = _duanyan_quintic_plan(from, to, T);\n");
                    self.output.push_str("    uint32_t dt_ms = (uint32_t)(T * 1000.0f / (steps - 1));\n");
                    self.output.push_str("    for (int i = 0; i < steps; i++) {\n");
                    self.output.push_str("        float t = T * (float)i / (float)(steps - 1);\n");
                    self.output.push_str("        _duanyan_traj_point_t p = _duanyan_quintic_sample(h, t);\n");
                    self.output.push_str("        duanyan_joint_write(idx, p.pos, (uint16_t)dt_ms);\n");
                    self.output.push_str("        vTaskDelay(pdMS_TO_TICKS(dt_ms));\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                // v1.6 P3: duanyan:modeb Mode-B 线协议直发 (镜像 DuanYanX mode_b_proto, 帧 AA 55 LEN CMD PL CRC8)
                if self.hw_imports.iter().any(|s| s.starts_with("modeb:")) {
                    self.output.push_str("/* DUANYAN Mode-B wire protocol (v1.6 P3: 镜像 DuanYanX mode_b_proto; 骨架不替换真机 uart_linkm6 位啮时序) */\n");
                    self.output.push_str("#ifndef DUANYAN_MODEB_UART\n#define DUANYAN_MODEB_UART UART_NUM_0\n#endif\n");
                    self.output.push_str("#define DUANYAN_MODEB_HEAD0 0xAA\n#define DUANYAN_MODEB_HEAD1 0x55\n#define DUANYAN_MODEB_MAXPL 64\n");
                    self.output.push_str("/* CRC8 poly 0x07 init 0 — 与真机固件 crc8() 逐位等价 */\n");
                    self.output.push_str("static inline uint8_t _duanyan_modeb_crc8(const uint8_t *d, int n) {\n");
                    self.output.push_str("    uint8_t crc = 0;\n");
                    self.output.push_str("    for (int i = 0; i < n; i++) {\n");
                    self.output.push_str("        crc ^= d[i];\n");
                    self.output.push_str("        for (int b = 0; b < 8; b++) crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x07) : (uint8_t)(crc << 1);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    return crc;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 组帧 + 8×'U' 校准突发 + UART 写: 返回帧总字节数 (5+len, 不含前导) */\n");
                    self.output.push_str("static inline int _duanyan_modeb_send(uint8_t cmd, const uint8_t *pl, uint8_t len) {\n");
                    self.output.push_str("    uint8_t buf[5 + DUANYAN_MODEB_MAXPL];\n");
                    self.output.push_str("    if (len > DUANYAN_MODEB_MAXPL) len = DUANYAN_MODEB_MAXPL;\n");
                    self.output.push_str("    buf[0] = DUANYAN_MODEB_HEAD0; buf[1] = DUANYAN_MODEB_HEAD1; buf[2] = len; buf[3] = cmd;\n");
                    self.output.push_str("    for (int i = 0; i < len; i++) buf[4 + i] = pl[i];\n");
                    self.output.push_str("    buf[4 + len] = _duanyan_modeb_crc8(&buf[2], 2 + len);   /* CRC 覆盖 LEN,CMD,PAYLOAD */\n");
                    self.output.push_str("    for (int i = 0; i < 8; i++) uart_write_bytes(DUANYAN_MODEB_UART, \"U\", 1);  /* 位啮自校准突发 */\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_MODEB_UART, (const char*)buf, 5 + len);\n");
                    self.output.push_str("    return 5 + len;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x20 关节设置: payload [joint][ang_hi][ang_lo][lo_hi][lo_lo][hi_hi][hi_lo] */\n");
                    self.output.push_str("static inline int _duanyan_modeb_joint_set(uint8_t joint, uint16_t ang, uint16_t lo, uint16_t hi) {\n");
                    self.output.push_str("    uint8_t pl[7] = { joint, (uint8_t)(ang>>8), (uint8_t)(ang&0xFF), (uint8_t)(lo>>8), (uint8_t)(lo&0xFF), (uint8_t)(hi>>8), (uint8_t)(hi&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x20, pl, 7);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x22 四关节归中: ARM_HOME_ANGLE=900(0x0384) — 黄金帧字节固定 */\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_home(void) {\n");
                    self.output.push_str("    uint8_t pl[13] = { 4, 0,0x03,0x84, 1,0x03,0x84, 2,0x03,0x84, 3,0x03,0x84 };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x22, pl, 13);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x30 臂总线同步写: items = n×[id,pw_hi,pw_lo,t_hi,t_lo] 已排布, n≤8 */\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_sync(const uint8_t *items, uint8_t n) {\n");
                    self.output.push_str("    uint8_t pl[1 + 8*5];\n");
                    self.output.push_str("    if (n == 0 || n > 8) return 0;\n");
                    self.output.push_str("    pl[0] = n;\n");
                    self.output.push_str("    for (int i = 0; i < n*5; i++) pl[1+i] = items[i];\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x30, pl, 1 + n*5);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x01 心跳: payload = uptime 秒 LE 4 字节 */\n");
                    self.output.push_str("static inline int _duanyan_modeb_heartbeat(uint32_t uptime) {\n");
                    self.output.push_str("    uint8_t pl[4] = { (uint8_t)(uptime&0xFF), (uint8_t)((uptime>>8)&0xFF), (uint8_t)((uptime>>16)&0xFF), (uint8_t)((uptime>>24)&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x01, pl, 4);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoAvr => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (Arduino AVR) */\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  duanyan_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (Arduino) */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (Arduino) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM helpers (Arduino AVR, 8-bit duty 0-255) */\n");
                    self.output.push_str("static inline void duanyan_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers (Arduino AVR, 10-bit) */\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (Arduino AVR)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (Arduino Servo.h) */\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 12\n");
                    self.output.push_str("static Servo _duanyan_servos[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_pins[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _duanyan_servo_count++;\n");
                    self.output.push_str("    _duanyan_servo_pins[id] = pin;\n");
                    self.output.push_str("    _duanyan_servos[id].attach(pin, min_us, max_us);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pins[i] == pin) { _duanyan_servos[i].write(angle); return; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (Arduino AVR) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_ena[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    _duanyan_motor_ena[id] = ena;\n");
                    self.output.push_str("    pinMode(in1, OUTPUT);\n");
                    self.output.push_str("    pinMode(in2, OUTPUT);\n");
                    self.output.push_str("    pinMode(ena, OUTPUT);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], HIGH);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], HIGH);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], speed);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], 0);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoArm => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  duanyan_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM helpers (Arduino ARM, 12-bit duty 0-4095) */\n");
                    self.output.push_str("static inline void duanyan_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers (Arduino ARM, 12-bit) */\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (Arduino ARM)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (Arduino ARM Servo.h) */\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 12\n");
                    self.output.push_str("static Servo _duanyan_servos[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_pins[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _duanyan_servo_count++;\n");
                    self.output.push_str("    _duanyan_servo_pins[id] = pin;\n");
                    self.output.push_str("    _duanyan_servos[id].attach(pin, min_us, max_us);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pins[i] == pin) { _duanyan_servos[i].write(angle); return; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (Arduino ARM) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_ena[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    _duanyan_motor_ena[id] = ena;\n");
                    self.output.push_str("    pinMode(in1, OUTPUT);\n");
                    self.output.push_str("    pinMode(in2, OUTPUT);\n");
                    self.output.push_str("    pinMode(ena, OUTPUT);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], HIGH);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], HIGH);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], speed);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], 0);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::Stm32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (STM32 HAL) */\n");
                    self.output.push_str("static GPIO_TypeDef* duanyan_gpio_port(int pin) {\n");
                    self.output.push_str("    if (pin >= 16) return GPIOB;\n");
                    self.output.push_str("    return GPIOA;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint16_t duanyan_gpio_pin_mask(int pin) {\n");
                    self.output.push_str("    return (uint16_t)(1 << (pin & 0xF));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = duanyan_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_OUTPUT_PP;\n");
                    self.output.push_str("    gpio.Pull = GPIO_NOPULL;\n");
                    self.output.push_str("    gpio.Speed = GPIO_SPEED_FREQ_LOW;\n");
                    self.output.push_str("    HAL_GPIO_Init(duanyan_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = duanyan_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_INPUT;\n");
                    self.output.push_str("    gpio.Pull = GPIO_PULLUP;\n");
                    self.output.push_str("    HAL_GPIO_Init(duanyan_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(duanyan_gpio_port(pin), duanyan_gpio_pin_mask(pin), GPIO_PIN_SET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(duanyan_gpio_port(pin), duanyan_gpio_pin_mask(pin), GPIO_PIN_RESET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_gpio_read(int pin) {\n");
                    self.output.push_str("    return HAL_GPIO_ReadPin(duanyan_gpio_port(pin), duanyan_gpio_pin_mask(pin));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (STM32 HAL) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { HAL_Delay(ms); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (STM32 HAL) */\n");
                    self.output.push_str("static UART_HandleTypeDef _duanyan_huart;\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) {\n");
                    self.output.push_str("    _duanyan_huart.Instance = USART1;\n");
                    self.output.push_str("    _duanyan_huart.Init.BaudRate = baud;\n");
                    self.output.push_str("    _duanyan_huart.Init.WordLength = UART_WORDLENGTH_8B;\n");
                    self.output.push_str("    _duanyan_huart.Init.StopBits = UART_STOPBITS_1;\n");
                    self.output.push_str("    _duanyan_huart.Init.Parity = UART_PARITY_NONE;\n");
                    self.output.push_str("    _duanyan_huart.Init.Mode = UART_MODE_TX_RX;\n");
                    self.output.push_str("    HAL_UART_Init(&_duanyan_huart);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) {\n");
                    self.output.push_str("    HAL_UART_Transmit(&_duanyan_huart, (uint8_t*)str, strlen(str), HAL_MAX_DELAY);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM helpers (STM32 TIM, 16-bit duty) */\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int pin, int duty) {\n");
                    self.output.push_str("    /* User must configure TIM per pin; this is a placeholder */\n");
                    self.output.push_str("    (void)pin; (void)duty;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers (STM32 HAL) */\n");
                    self.output.push_str("static ADC_HandleTypeDef _duanyan_hadc;\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) {\n");
                    self.output.push_str("    ADC_ChannelConfTypeDef sConfig = {0};\n");
                    self.output.push_str("    sConfig.Channel = channel;\n");
                    self.output.push_str("    HAL_ADC_ConfigChannel(&_duanyan_hadc, &sConfig);\n");
                    self.output.push_str("    HAL_ADC_Start(&_duanyan_hadc);\n");
                    self.output.push_str("    HAL_ADC_PollForConversion(&_duanyan_hadc, 100);\n");
                    self.output.push_str("    int val = HAL_ADC_GetValue(&_duanyan_hadc);\n");
                    self.output.push_str("    HAL_ADC_Stop(&_duanyan_hadc);\n");
                    self.output.push_str("    return val;\n");
                    self.output.push_str("}\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (STM32 TIM HAL)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (STM32 TIM2 PWM 50Hz) */\n");
                    self.output.push_str("static TIM_HandleTypeDef _duanyan_servo_htim;\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 4\n");
                    self.output.push_str("static int _duanyan_servo_pin[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_min[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_max[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _duanyan_servo_count++;\n");
                    self.output.push_str("    _duanyan_servo_pin[id] = pin;\n");
                    self.output.push_str("    _duanyan_servo_min[id] = min_us;\n");
                    self.output.push_str("    _duanyan_servo_max[id] = max_us;\n");
                    self.output.push_str("    /* Configure TIM2 for 50Hz PWM (20ms period) */\n");
                    self.output.push_str("    _duanyan_servo_htim.Instance = TIM2;\n");
                    self.output.push_str("    _duanyan_servo_htim.Init.Prescaler = 72 - 1; /* 72MHz/72 = 1MHz tick */\n");
                    self.output.push_str("    _duanyan_servo_htim.Init.Period = 20000 - 1; /* 1MHz/20000 = 50Hz */\n");
                    self.output.push_str("    _duanyan_servo_htim.Init.CounterMode = TIM_COUNTERMODE_UP;\n");
                    self.output.push_str("    HAL_TIM_PWM_Init(&_duanyan_servo_htim);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    int id = -1;\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pin[i] == pin) { id = i; break; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (id < 0) return;\n");
                    self.output.push_str("    if (angle < 0) angle = 0;\n");
                    self.output.push_str("    if (angle > 180) angle = 180;\n");
                    self.output.push_str("    int pulse_us = _duanyan_servo_min[id] + (angle * (_duanyan_servo_max[id] - _duanyan_servo_min[id])) / 180;\n");
                    self.output.push_str("    TIM_OC_InitTypeDef oc = {0};\n");
                    self.output.push_str("    oc.OCMode = TIM_OCMODE_PWM1;\n");
                    self.output.push_str("    oc.Pulse = pulse_us;\n");
                    self.output.push_str("    HAL_TIM_PWM_ConfigChannel(&_duanyan_servo_htim, &oc, TIM_CHANNEL_1);\n");
                    self.output.push_str("    HAL_TIM_PWM_Start(&_duanyan_servo_htim, TIM_CHANNEL_1);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (STM32 HAL GPIO + TIM) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    duanyan_gpio_output(in1);\n");
                    self.output.push_str("    duanyan_gpio_output(in2);\n");
                    self.output.push_str("    (void)ena; /* PWM config via TIM */\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        duanyan_gpio_high(_duanyan_motor_in1[id]);\n");
                    self.output.push_str("        duanyan_gpio_low(_duanyan_motor_in2[id]);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        duanyan_gpio_low(_duanyan_motor_in1[id]);\n");
                    self.output.push_str("        duanyan_gpio_high(_duanyan_motor_in2[id]);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    /* TODO: set TIM duty cycle for speed */\n");
                    self.output.push_str("    (void)speed;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    duanyan_gpio_low(_duanyan_motor_in1[id]);\n");
                    self.output.push_str("    duanyan_gpio_low(_duanyan_motor_in2[id]);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::RaspberryPi => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (Linux sysfs) */\n");
                    self.output.push_str("static inline void duanyan_gpio_export(int pin) {\n");
                    self.output.push_str("    char buf[64];\n");
                    self.output.push_str("    int fd = open(\"/sys/class/gpio/export\", O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) {\n");
                    self.output.push_str("        int n = snprintf(buf, sizeof(buf), \"%d\", pin);\n");
                    self.output.push_str("        write(fd, buf, n);\n");
                    self.output.push_str("        close(fd);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) {\n");
                    self.output.push_str("    duanyan_gpio_export(pin);\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/direction\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"out\", 3); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"1\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"0\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_gpio_read(int pin) {\n");
                    self.output.push_str("    char path[128], val;\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_RDONLY);\n");
                    self.output.push_str("    if (fd >= 0) { read(fd, &val, 1); close(fd); return val - '0'; }\n");
                    self.output.push_str("    return -1;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (Linux usleep) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { usleep(ms * 1000); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (Linux serial) */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) { (void)baud; /* TODO: termios */ }\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) { printf(\"%s\", str); }\n\n");
                }
            }
            _ => {} // 其他平台后续实现
        }
    }

}