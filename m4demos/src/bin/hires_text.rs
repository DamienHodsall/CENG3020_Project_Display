//! Attributed text display.
//!
//! This shows a mostly static screen of colored text, with some dynamic
//! elements for fun.
//!
//! # Theory of operation
//!
//! We allocate a static buffer, `TEXT_BUF`, to hold attributed text. Our
//! rasterizer expects that buffer to contain values of type `AChar`, for
//! *attributed char*.
//!
//! Because we want to update the text from the application loop, but read it
//! during scanout, we enclose the buffer in a `SpinLock`. Before updating the
//! text in the application loop, we `sync_to_vblank` to ensure that we're not
//! racing scanout.
//!
//! At startup, at the top of `main`, the demo fills the text buffer with text.
//! It then activates the display driver, giving it a raster callback and a main
//! loop.
//!
//! The raster callback uses `m4vga::rast::text_10x16` to draw the top 592 lines
//! of the display, using the standard font, and then `m4vga::rast::solid_color`
//! to draw the partial last line. During the rendering of the text part of the
//! display, it locks `TEXT_BUF` on every horizontal retrace -- this is pretty
//! cheap, and won't race the application loop because we only call the raster
//! callback outside of the vertical blanking interval.
//!
//! The application loop calls `sync_to_vblank` every iteration and then writes
//! an updated frame number into the `TEXT_BUF`. Note that we can use `write!`
//! here despite `no_std`; we don't have to write our own numeric formatting
//! code, which is great.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use stm32f4;
use stm32f4::stm32f407::interrupt;
use stm32f4::stm32f407 as device;

use font_10x16;
use m4vga::rast::text_10x16::{self, AChar};
use m4vga::util::spin_lock::SpinLock;

const COLS: usize = 80;
const ROWS: usize = 37;

const WHITE: u8 = 0b11_11_11;
const BLACK: u8 = 0b00_00_00;
const DK_GRAY: u8 = 0b01_01_01;
const RED: u8 = 0b00_00_11;
const BLUE: u8 = 0b11_00_00;

static TEXT_BUF: SpinLock<[AChar; COLS * ROWS]> =
    SpinLock::new([AChar::from_ascii_char(0); COLS * ROWS]);

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    {
        // Type some stuff into the buffer.
        let mut c = TEXT_BUF.try_lock().unwrap();
        let mut c = Cursor::new(&mut *c);
        screen_error(&mut c);
        // c.fg = WHITE;
        // c.bg = DK_GRAY;
        // c.puts(b"800x600 Attributed Text Demo\n");
        // c.bg = BLACK;
        // c.puts(b"10x16 point characters in an 80x37 grid, with ");
        // c.fg = RED;
        // c.puts(b"foreground");
        // c.fg = WHITE;
        // c.puts(b" and ");
        // c.bg = BLUE;
        // c.puts(b"background");
        // c.bg = BLACK;
        // c.puts(b" colors.\n");
        // c.bg = 0b10_00_00;
        // c.puts(
            // br#"
       // Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nam ut
       // tellus quam. Cras ornare facilisis sollicitudin. Quisque quis
       // imperdiet mauris. Proin malesuada nibh dolor, eu luctus mauris
       // ultricies vitae. Interdum et malesuada fames ac ante ipsum primis
       // in faucibus. Aenean tincidunt viverra ultricies. Quisque rutrum
       // vehicula pulvinar.
// 
       // Etiam commodo dui quis nibh dignissim laoreet. Aenean erat justo,
       // hendrerit ac adipiscing tempus, suscipit quis dui. Vestibulum ante
       // ipsum primis in faucibus orci luctus et ultrices posuere cubilia
       // Curae; Proin tempus bibendum ultricies. Etiam sit amet arcu quis
       // diam dictum suscipit eu nec odio. Donec cursus hendrerit porttitor.
       // Suspendisse ornare, diam vitae faucibus dictum, leo enim vestibulum
       // neque, id tempor tellus sem pretium lectus. Maecenas nunc nisl,
       // aliquam non quam at, vulputate lacinia dolor. Vestibulum nisi orci,
       // viverra ut neque semper, imperdiet laoreet ligula. Nullam venenatis
       // orci eget nibh egestas, sit amet sollicitudin erat cursus.
// 
       // Nullam id ornare tellus, vel porta lectus. Suspendisse pretium leo
       // enim, vel elementum nibh feugiat non. Etiam non vulputate quam, sit
       // amet semper ante. In fermentum imperdiet sem non consectetur. Donec
       // egestas, massa a fermentum viverra, lectus augue hendrerit odio,
       // vitae molestie nibh nunc ut metus. Nulla commodo, lacus nec
       // interdum dignissim, libero dolor consequat mi, non euismod velit
       // sem nec dui. Praesent ligula turpis, auctor non purus eu,
       // adipiscing pellentesque felis."#,
        // );
        // c.putc(b'\n');
    }

    let mut cp = cortex_m::peripheral::Peripherals::take().unwrap();
    let p = device::Peripherals::take().unwrap();

    // allow clock to access gpioc
    p.RCC.ahb1enr.modify(|_, w| {w.gpiocen().enabled()});
    // turn on gpioc input for pins 7 8 9
    p.GPIOC.moder.modify(|_, w| w.moder7().input().moder8().input().moder9().input());
    // simplify input as idr
    let input = &p.GPIOC.idr;

    let mut s0: u8 = 0;

    // Give the driver its hardware resources...
    m4vga::init(
        cp.NVIC,
        &mut cp.SCB,
        p.FLASH,
        &p.DBG,
        p.RCC,
        p.GPIOB,
        p.GPIOE,
        p.TIM1,
        p.TIM3,
        p.TIM4,
        p.DMA2,
        )
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            // The raster callback is invoked on every horizontal retrace to
            // provide new pixels.
            |ln, tgt, ctx, _| {
                if ln < 592 {
                    text_10x16::unpack(
                        &*TEXT_BUF.try_lock().expect("rast buf access"),
                        font_10x16::FONT.as_glyph_slices(),
                        &mut **tgt,
                        ln,
                        COLS,
                    );
                    ctx.target_range = 0..COLS * text_10x16::GLYPH_COLS;
                } else {
                    // There's a partial 38th line visible on the display.
                    // Trying to display it will panic by going out of range on
                    // the 80x37 buffer. Instead, we'll just black it out:
                    m4vga::rast::solid_color_fill(tgt, ctx, 800, 0);
                    // Save some CPU while we're at it by not invoking the
                    // callback again this frame.
                    ctx.repeat_lines = 600 - 592;
                }
            },
            // This closure contains the main loop of the program.
            |vga| {
                // Enable outputs. The driver doesn't do this for you in case
                // you want to set up some graphics before doing so.

                vga.video_on();
                // let mut frame_no = 0;
                // Spin forever!
                loop {
                    use core::fmt::Write;

                    vga.sync_to_vblank();
                    let mut buf = TEXT_BUF.try_lock().expect("app buf access");
                    let mut c = Cursor::new(&mut *buf);
                    // c.goto(36, 0);
                    // c.bg = 0;
                    // c.fg = 0b00_11_00;
                    // write!(&mut c, "Welcome to frame {}", frame_no).unwrap();
                    // frame_no += 1;

                    let s = ((input.read().idr7().bit() as u8) << 0) + ((input.read().idr8().bit() as u8) << 1) + ((input.read().idr9().bit() as u8) << 2);

                    // test if the screen is being updated every cycle or on change
                    // point will show if it works as expected
                    // c.goto(36,0);
                    // c.bg = RED;
                    // c.fg = BLUE;
                    // c.putc(b'*');

                    if s0 != s {
                        match s {
                            0b000 => screen_error(&mut c),
                            0b001 => screen_start(&mut c),
                            0b010 => screen_paying(&mut c),
                            0b011 => screen_confirm(&mut c),
                            0b100 => screen_line1(&mut c),
                            0b101 => screen_line2(&mut c),
                            0b110 => screen_thanks(&mut c),
                            _ => screen_error(&mut c),
                        }
                    }

                    s0 = s + 0;

                    c.bg = RED;
                    c.fg = WHITE;
                    c.goto(35, 77);
                    write!(&mut c, "{:03b}", s);
                }
            },
        )
}

/// A simple cursor wrapping a text buffer. Provides terminal-style operations.
struct Cursor<'a> {
    buf: &'a mut [AChar; COLS * ROWS],
    row: usize,
    col: usize,
    fg: m4vga::Pixel,
    bg: m4vga::Pixel,
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a mut [AChar; COLS * ROWS]) -> Self {
        Cursor {
            buf,
            row: 0,
            col: 0,
            fg: 0xFF,
            bg: 0b100000,
        }
    }

    /// Types a character terminal-style and advances the cursor. `'\n'` is
    /// interpreted as carriage return plus line feed.
    pub fn putc(&mut self, c: u8) {
        match c {
            b'\n' => {
                let pos = self.row * COLS + self.col;
                let end_of_line = (pos + (COLS - 1)) / COLS * COLS;
                for p in &mut self.buf[pos..end_of_line] {
                    *p = AChar::from_ascii_char(b' ')
                        .with_foreground(self.fg)
                        .with_background(self.bg)
                }
                self.col = 0;
                self.row += 1;
            }
            _ => {
                self.buf[self.row * COLS + self.col] =
                    AChar::from_ascii_char(c)
                        .with_foreground(self.fg)
                        .with_background(self.bg);
                self.col += 1;
                if self.col == COLS {
                    self.col = 0;
                    self.row += 1;
                }
            }
        }
    }

    /// Types each character from an ASCII slice.
    pub fn puts(&mut self, s: &[u8]) {
        for c in s {
            self.putc(*c)
        }
    }

    /// Repositions the cursor.
    pub fn goto(&mut self, row: usize, col: usize) {
        assert!(row < ROWS);
        assert!(col < COLS);
        self.row = row;
        self.col = col;
    }

    /// Clears the buffer.
    pub fn clear(&mut self) {
        self.goto(0,0);
        for _i in 0..COLS*ROWS {
            self.putc(b' ');
        }
    }
}

/// Allows use of a `Cursor` in formatting and `write!`.
impl<'a> core::fmt::Write for Cursor<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            let c = c as u32;
            self.putc(c as u8);
        }

        Ok(())
    }
}

/// Wires up the PendSV handler expected by the driver.
#[cortex_m_rt::exception]
#[link_section = ".ramcode"]
fn PendSV() {
    m4vga::pendsv_raster_isr()
}

/// Wires up the TIM3 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM3() {
    m4vga::tim3_shock_isr()
}

/// Wires up the TIM4 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM4() {
    m4vga::tim4_horiz_isr()
}

// This is all my code

// fn read_bits(gpioa: &device::GPIOA) -> u8 {
//     ((gpioa.idr.read().idr7().bit() as u8) << 0) + ((gpioa.idr.read().idr8().bit() as u8) << 1) + ((gpioa.idr.read().idr9().bit() as u8) << 2)
// }

// fn surround_text(c: &mut Cursor, t: &str, y: usize, x: usize) {
//     let n: usize = t.find('\n').expect("strings will contain \\n") + 3;
//     c.goto(y, x);
//     for _ in (1..n) { c.putc(b' ') }
//     for line in t.lines() {
//         let y = y + 1;
//         c.goto(y, x);
//         c.putc(b' ');
//         c.puts(line.as_bytes());
//         c.putc(b' ');
//     }
//     c.goto(y + 1, x);
//     for _ in (1..n) { c.putc(b' ') }

//     // let l: usize = t.find('\n').expect("strings will contain \\n");
//     // let t = t.as_bytes();
//     // c.goto(y, x);
//     // for _ in (1..l+2) { c.putc(b' '); }
//     // for i in (0..t.len()) {
//         // if (i%l == 0) {
//             // c.putc(b' ');
//             // let y = y + 1;
//             // c.goto(y, x);
//             // c.putc(b' ');
//         // }
//         // else {
//             // c.putc(t[i]);
//         // }
//     // }
//     // for _ in (1..l+2) { c.putc(b' '); }
// }

fn screen_error(c: &mut Cursor) {

    // reset
    c.bg = BLUE;
    c.fg = WHITE;
    c.clear();

    // title
    c.goto(0,0);
    c.bg = RED;
    c.puts(b" \n");
    c.puts(b"                                     ERROR                                      ");
    c.puts(b" \n");

    c.bg = BLACK;
}

fn screen_start(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // message
    c.bg = BLUE;
    c.goto(16,35);
    c.puts(b"           ");
    c.goto(17,35);
    c.puts(b" Press any ");
    c.goto(18,35);
    c.puts(b"  button   ");
    c.goto(19,35);
    c.puts(b" to start! ");
    c.goto(20,35);
    c.puts(b"           ");

    c.bg = BLACK;
}

fn screen_paying(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // title
    c.bg = BLUE;
    c.goto(0,0);
    c.puts(b" \n");
    c.puts(b"                                    Payment                                     ");
    c.puts(b" \n");

    // message
    c.goto(17,35);
    c.puts(b"          ");
    c.goto(18,35);
    c.puts(b"  Please  ");
    c.goto(19,35);
    c.puts(b" pay now. ");
    c.goto(20,35);
    c.puts(b"          ");

    c.bg = BLACK;
}

fn screen_confirm(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // title
    c.bg = BLUE;
    c.goto(0,0);
    c.puts(b" \n");
    c.puts(b"                                  Confirmation                                  ");
    c.puts(b" \n");

    // message
    c.goto(17,35);
    c.puts(b"           ");
    c.goto(18,35);
    c.puts(b"  Do you   ");
    c.goto(19,35);
    c.puts(b"  want to  ");
    c.goto(20,35);
    c.puts(b" continue? ");
    c.goto(21,35);
    c.puts(b"           ");

    // option 1 (top left)
    c.bg = 0b00_10_00;
    c.goto(11,0);
    c.puts(b"     ");
    c.goto(12,0);
    c.puts(b" YES ");
    c.goto(13,0);
    c.puts(b"     ");

    // option 2 (bottom left)
    c.bg = 0b00_00_10;
    c.goto(24,0);
    c.puts(b"     ");
    c.goto(25,0);
    c.puts(b" NO  ");
    c.goto(26,0);
    c.puts(b"     ");

    c.bg = BLACK;
}

fn screen_line1(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // title
    c.bg = BLUE;
    c.goto(0,0);
    c.puts(b" \n");
    c.puts(b"                                     Line 1                                     ");
    c.puts(b" \n");

    c.goto(16,35);
    c.puts(b"           ");
    c.goto(17,35);
    c.puts(b" Choose a  ");
    c.goto(18,35);
    c.puts(b" ticket to ");
    c.goto(19,35);
    c.puts(b" purchase  ");
    c.goto(20,35);
    c.puts(b"           ");

    // option 1 (top left)
    c.goto(10,0);
    c.puts(b"      ");
    c.goto(11,0);
    c.puts(b"   A  ");
    c.goto(12,0);
    c.puts(b"      ");

    // option 2 (top right)
    c.goto(10,74);
    c.puts(b"      ");
    c.goto(11,74);
    c.puts(b"  B   ");
    c.goto(12,74);
    c.puts(b"      ");

    // option 3 (bottom left)
    c.goto(25,0);
    c.puts(b"      ");
    c.goto(26,0);
    c.puts(b" QUIT ");
    c.goto(27,0);
    c.puts(b"      ");

    // option (bottom right)
    c.goto(25,74);
    c.puts(b"      ");
    c.goto(26,74);
    c.puts(b" NEXT ");
    c.goto(27,74);
    c.puts(b"      ");
}

fn screen_line2(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // title
    c.bg = BLUE;
    c.goto(0,0);
    c.puts(b" \n");
    c.puts(b"                                     Line 2                                     ");
    c.puts(b" \n");

    c.goto(16,35);
    c.puts(b"           ");
    c.goto(17,35);
    c.puts(b" Choose a  ");
    c.goto(18,35);
    c.puts(b" ticket to ");
    c.goto(19,35);
    c.puts(b" purchase  ");
    c.goto(20,35);
    c.puts(b"           ");

    // option 1 (top left)
    c.goto(10,0);
    c.puts(b"      ");
    c.goto(11,0);
    c.puts(b"   C  ");
    c.goto(12,0);
    c.puts(b"      ");

    // option 2 (top right)
    c.goto(10,74);
    c.puts(b"      ");
    c.goto(11,74);
    c.puts(b"  D   ");
    c.goto(12,74);
    c.puts(b"      ");

    // option 3 (bottom left)
    c.goto(25,0);
    c.puts(b"      ");
    c.goto(26,0);
    c.puts(b" PREV ");
    c.goto(27,0);
    c.puts(b"      ");

    // option (bottom right)
    c.goto(25,74);
    c.puts(b"      ");
    c.goto(26,74);
    c.puts(b" NEXT ");
    c.goto(27,74);
    c.puts(b"      ");
}

fn screen_line3(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // title
    c.bg = BLUE;
    c.goto(0,0);
    c.puts(b" \n");
    c.puts(b"                                     Line 3                                     ");
    c.puts(b" \n");

    c.goto(16,35);
    c.puts(b"           ");
    c.goto(17,35);
    c.puts(b" Choose a  ");
    c.goto(18,35);
    c.puts(b" ticket to ");
    c.goto(19,35);
    c.puts(b" purchase  ");
    c.goto(20,35);
    c.puts(b"           ");

    // option 1 (top left)
    c.goto(10,0);
    c.puts(b"      ");
    c.goto(11,0);
    c.puts(b"   E  ");
    c.goto(12,0);
    c.puts(b"      ");

    // option 2 (top right)
    c.goto(10,74);
    c.puts(b"      ");
    c.goto(11,74);
    c.puts(b"  F   ");
    c.goto(12,74);
    c.puts(b"      ");

    // option 3 (bottom left)
    c.goto(25,0);
    c.puts(b"      ");
    c.goto(26,0);
    c.puts(b" PREV ");
    c.goto(27,0);
    c.puts(b"      ");

    // option (bottom right)
    c.goto(25,74);
    c.puts(b"      ");
    c.goto(26,74);
    c.puts(b" QUIT ");
    c.goto(27,74);
    c.puts(b"      ");
}

fn screen_thanks(c: &mut Cursor) {

    // reset
    c.bg = DK_GRAY;
    c.fg = WHITE;
    c.clear();

    // title
    c.bg = BLUE;
    c.goto(0,0);
    c.puts(b" \n");
    c.puts(b"                                    Thank You                                   ");
    c.puts(b" \n");

    // message
    c.bg = 0b00_01_00; // Green
    c.fg = BLACK;
    c.goto(17,34);
    c.puts(b"            ");
    c.goto(18,34);
    c.puts(b" Thanks for ");
    c.goto(19,34);
    c.puts(b" travelling ");
    c.goto(20,34);
    c.puts(b"  with us!  ");
    c.goto(21,34);
    c.puts(b"            ");
}
