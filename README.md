# Brachiograph

Software for controlling a [brachiograph](https://www.brachiograph.art/explanation/index.html)
using embedded rust. A brachiograph is a cheap and easy-to-make pen plotter; see the link
above for instructions on how to build one.

This repository exists because I wanted to build a brachiograph but couldn't obtain a
Raspberry Pi (or similar), so I used an `stm32f103` "blue pill" development board instead.
The software here lets the `stm32f103` control a brachiograph and expose a simple USB serial
interface. Instructions may be posted at some point; in the meantime, if you're interested feel
free to open an issue and ask.

The `brachiograph` crate contains various reusable geometric routines, including code
for calculating elbow and shoulder angles from pen positions. The `runner` crate
contains binaries for the `stm32f103` development board.