# GSim-RS

![GSim Demo](./media/demo.gif)

A G-code simulator written in Rust.
Parses, interprets, manages machine state and simulates the toolpaths.
The control interface is built in **Ratatui** and the simulation is done using **WGPU**.

---

**G-code** or **Geometric code** is the language used to encode instructions for a CNC
machine. These instructions cause the machine to move in extremely precise & controlled
manner to make all types of geometries.

<br>

## Architecture

I have never done system diagrams for personal projects,
but I feel like this one warrants one as there are **A LOT** of moving parts.

Here is an **extremely high level** view of the architecture:
![An extremely high level architecture diagram of GSim](./media/arch.svg)

<br>

## Highlights

- **TUI** and **GUI** run on different threads using a feedback cycle, ensuring that both the
  interfaces are in sync.
- **Single execution** of blocks is supported, allowing stepping through blocks.
- **Rapid** and **Feed** moves are differentiated visually in the simulation.
- **Isometric** and **Top** simulation views can be switched between, at runtime.
- **Machine boundary box** can be activated to visualize the extremes of machine travels.
- Smooth simulation of **adaptive** or **dynamic** toolpaths is ensured by batching up tiny moves
  before rendering them to the frame. This is bypassed on **single mode on** to give the user
  instant visual feedback, thus rendering each move irrespective of the move length.
- Parsing and interpretation only happen during the first cycle and are **cached**. This makes
  subsequent cycles more efficient.
- **Overtravel** is calculated before each move and an error is raised if the move will
  cause the machine to go off the boundary.
- Both **metric** & **imperial** units can be used.

<br>

## Quick Start

<details>
<summary>Dependencies</summary>

<br>

- [anyhow](https://docs.rs/anyhow/latest/anyhow/index.html)
- [clap](https://docs.rs/clap/latest/clap/)
- [pollster](https://docs.rs/pollster/latest/pollster/)
- [ratatui](https://docs.rs/ratatui/latest/ratatui/)
- [wgpu](https://docs.rs/wgpu/latest/wgpu/index.html)
- [winit](https://docs.rs/winit/latest/winit/)
- [bytemuck](https://docs.rs/bytemuck/latest/bytemuck/)
- [env_logger](https://docs.rs/env_logger/latest/env_logger/)
- [log](https://docs.rs/log/latest/log/)
- [thiserror](https://docs.rs/thiserror/latest/thiserror/)

</details>

### Install with `cargo`:

1. Install `cargo` package manager from [crates.io](https://crates.io).

2. Run:
```bash
cargo install gsim-rs
```

<br>

## Supported Codes

### G Codes
| **Code** | **Description** |
| :-: | :-: |
| **G00** | Rapid Move |
| **G01** | Feed Move |
| **G02** | Clockwise Arc Move |
| **G03** | Anti-Clockwise Arc Move |
| **G04** | Dwell |
| **G17** | XY Plane Selection |
| **G18** | XZ Plane Selection |
| **G19** | YZ Plane Selection |
| **G20** | Imperial Mode |
| **G21** | Metric Mode |
| **G40** | Cutter Comp Cancel |
| **G41** | Cutter Comp Left |
| **G42** | Cutter Comp Right |
| **G43** | Tool Length Comp Add |
| **G44** | Tool Length Comp Subtract |
| **G49** | Tool Length Comp Cancel |
| **G53** | Machine Position Move |
| **G54** | Workpiece Coordinate |
| **G80** | Cancel Canned Cycles |
| **G90** | Absolute Positioning |
| **G91** | Relative Positioning |
| **G94** | Feed Per Minute |
| **G95** | Feed Per Rev |
| **G98** | Initial Level Return |
| **G99** | Retract Level Return |

### M Codes
| **Code** | **Description** |
| :-: | :-: |
| **M00** | Cycle Pause |
| **M01** | Optional Cycle Pause |
| **M03** | Spindle On Forward |
| **M04** | Spindle On Reverse |
| **M05** | Spindle Stop |
| **M06** | Tool Change |
| **M08** | Coolant On |
| **M09** | Coolant Off |
| **M30** | Program End |

### Auxiliary Codes
| **Code** | **Description** |
| :-: | :-: |
| **D__** | Diameter Offset Register for **G40** & **G41** |
| **F__** | Feed Rate for **G01**, **G02** & **G03** |
| **H__** | Height Offset Register for **G43** & **G44** |
| **I__** | Relative Center of Arc in X axis for **G02** & **G03** |
| **J__** | Relative Center of Arc in Y axis for **G02** & **G03** |
| **K__** | Relative Center of Arc in Z axis for **G02** & **G03** |
| **N__** | Program Line Number |
| **O__** | Program Number |
| **P__** | Dwell Time in Milliseconds |
| **R__** | Arc Radius for **G02** & **G03** |
| **S__** | Spindle Speed for **M03** & **M04** |
| **T__** | Tool Number for **M06** |
| **X__** | X Axis Position for **G00**, **G01**, **G02**, **G03** & **G53** |
| **Y__** | Y Axis Position for **G00**, **G01**, **G02**, **G03** & **G53** |
| **Z__** | Z Axis Position for **G00**, **G01**, **G02**, **G03** & **G53** |

### Notes
- Default value for **G54** offset is **half of each machine axis travel**. Therefore each absolute move
  will be shifted to middle of the machine if activated.
- **G04 (Dwell)** does not block the threads and is ignored silently.
- **Cutter and Tool Length Compensations** do not alter the simulation and are thus ignored.

<br>

## References

**Most importantly**: [WGPU tutorial](https://sotrh.github.io/learn-wgpu/)

- Math for arc: [Math Stack Exchange](https://math.stackexchange.com/questions/1781438/finding-the-center-of-a-circle-given-two-points-and-a-radius-algebraically)
- Line vertex shader: [Github](https://github.com/KaNaDaAT/vega-webgpu/blob/main/src/shaders/line.wgsl)
- Points on an arc: [FreeMathHelp](https://www.freemathhelp.com/forum/threads/xy-points-on-an-arc.130791/)
- G-code: [Haas](https://www.haascnc.com/service/service-content/guide-procedures/what-are-g-codes.html#gsc.tab=0)
- Lexing & parsing: [Tomassetti](https://tomassetti.me/guide-parsing-algorithms-terminology/)
- Angle between two points on an arc: [Stackoverflow]( https://stackoverflow.com/questions/2994669/how-do-i-calculate-arc-angle-between-two-points-on-a-circle)
- Ratatui: [Docs](https://docs.rs/ratatui/latest/ratatui/)
- WGPU: [Docs](https://docs.rs/wgpu/latest/wgpu/)

