
#[macro_use] extern crate log;
extern crate simplelog;
// use std::os::windows::prelude;

use simplelog::{TermLogger, LevelFilter, TerminalMode, ColorChoice};

extern crate structopt;
use structopt::StructOpt;

extern crate humantime;
use humantime::Duration;

use streamdeck::{Colour, Error, Filter, ImageOptions, InputEvent, StreamDeck};
#[derive(StructOpt)]
#[structopt(name = "streamdeck-cli", about = "A CLI for the Elgato StreamDeck")]
struct Options {

    #[structopt(subcommand)]
    cmd: Commands,

    #[structopt(flatten)]
    filter: Filter,

    #[structopt(long = "log-level", default_value = "info")]
    /// Enable verbose logging
    level: LevelFilter,
}

#[derive(StructOpt)]
pub enum Commands {
    /// Reset the attached device
    Reset,
    /// Fetch the device firmware version
    Version,
    /// Set device display brightness
    SetBrightness{
        /// Brightness value from 0 to 100
        brightness: u8,
    },
    /// Fetch button states
    GetButtons {
        #[structopt(long)]
        /// Timeout for button reading
        timeout: Option<Duration>,

        #[structopt(long)]
        /// Read continuously
        continuous: bool,
    },
    /// Fetch input states (dials, buttons & touchscreen on SD Plus)
    GetInput {
        #[structopt(long)]
        /// Timeout for input reading
        timeout: Option<Duration>,

        #[structopt(long)]
        /// Read continuously
        continuous: bool,
        #[structopt(skip)]
        callback: Option<Box<dyn Fn(Vec<InputEvent>) -> Result<(), Error> + Send>>
    },
    /// Set button colours
    SetColour {
        /// Index of button to be set
        key: u8,

        #[structopt(flatten)]
        colour: Colour,
    },
    /// Set button images
    SetImage {
        /// Index of button to be set
        key: u8,

        /// Image file to be loaded
        file: String,

        #[structopt(flatten)]
        opts: ImageOptions,
    },
    /// Set touchscreen image
    SetTouchscreenImage {
        /// Image file to be loaded
        file: String,

        #[structopt(flatten)]
        opts: ImageOptions,
    }
}

fn main() {
    // Parse options
    let opts = Options::from_args();

    // Setup logging
    let mut config = simplelog::ConfigBuilder::new();
    config.set_time_level(LevelFilter::Off);

    TermLogger::init(opts.level, config.build(), TerminalMode::Mixed, ColorChoice::Auto).unwrap();

    // Connect to device
    let mut deck = match StreamDeck::connect(opts.filter.vid, opts.filter.pid, opts.filter.serial) {
        Ok(d) => d,
        Err(e) => {
            println!("Error connecting to streamdeck: {:?}", e);
            error!("Error connecting to streamdeck: {:?}", e);
            return
        }
    };

    let serial = deck.serial().unwrap();
    info!("Connected to device (vid: {:04x} pid: {:04x} serial: {})", 
            opts.filter.vid, opts.filter.pid, serial);

    // Run the command
    if let Err(e) = do_command(&mut deck, opts.cmd) {
        error!("Command error: {:?}", e);
    }
}

fn do_command(deck: &mut StreamDeck, cmd: Commands) -> Result<(), Error> {
    match cmd {
        Commands::Reset => {
            deck.reset()?;
        },
        Commands::Version => {
            let version = deck.version()?;
            info!("Firmware version: {}", version);
        }
        Commands::SetBrightness{brightness} => {
            deck.set_brightness(brightness)?;
        },
        Commands::GetButtons{timeout, continuous} => {
            loop {
                let buttons = deck.read_buttons(timeout.map(|t| *t ))?;
                info!("buttons: {:?}", buttons);

                if !continuous {
                    break
                }
            }
        },
        Commands::GetInput{timeout, continuous, callback} => {
            loop {
                let input = deck.read_input(timeout.map(|t| *t ))?;
                info!("input: {:?}", input);
                if let Some(cb) = &callback {
                    cb(input)?;
                }

                if !continuous {
                    break
                }
            }
        },
        Commands::SetColour{key, colour} => {
            info!("Setting key {} colour to: ({:?})", key, colour);
            deck.set_button_rgb(key, &colour)?;
        },
        Commands::SetImage{key, file, opts} => {
            info!("Setting key {} to image: {}", key, file);
            deck.set_button_file(key, &file, &opts)?;
        }

        Commands::SetTouchscreenImage{file, opts} => {
            info!("Setting touchscreen image: {}", file);
            let x: u16 = 0;
            let y: u16 = 50;
            let width: u16 = 800;
            let height: u16 = 120;
            deck.set_touchscreen_file(&file, x, y, width, height, &opts)?;
        }
    }

    Ok(())
}
