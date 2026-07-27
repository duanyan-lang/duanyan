// Espruino 风格测试代码
const LED = 2;
pinMode(LED, "output");
setInterval(function() {
    digitalWrite(LED, !digitalRead(LED));
}, 500);
console.log("LED blink started");
