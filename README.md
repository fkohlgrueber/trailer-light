# trailer-light

I built a fancy back light for my Thule Chariot Sport 1 bike trailer. The backlight consists of 58 RGB LEDs that are mounted on the top and sides of the trailer. I'm using an ESP32-C3 microcontroller to drive the LEDs. This repository contains the code used on the microcontroller.

## Usage

```
# install rust
# install espflash
cargo install espflash
# build and flash the program
cargo run --release
```

## Functionality

Currently, the code performs an animation at startup and then continuously displays a pulsating effect. 

TODO: Link to a video showing the effect here.