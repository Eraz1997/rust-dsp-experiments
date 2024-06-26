//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream configuration and that they
//! support the f32 sample format.
//!
//! Uses a delay of `LATENCY_MS` milliseconds in case the default input and output streams are not
//! precisely synchronised.

use clap::Parser;
use cpal::{BufferSize, FrameCount};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer};
use ringbuf::HeapRb;
use ringbuf::traits::Split;

// TODO: use dasp for more powerful DSP
// TODO: Add link to CPAL README for ASIO setup
// TODO: Add `cargo run --release --features jack (or asio)` to doc

enum Driver {
    Default,
    #[cfg(target_os = "windows")]
    Asio,
    #[cfg(target_os = "linux")]
    Jack,
}

struct Settings {
    buffer_size: i32,
    input_device: String,
    output_device: String,
    driver: Driver,
}

fn main() -> anyhow::Result<()> {
    // Get settings
    let settings = Settings {
        buffer_size: 128,
        input_device: "default".to_string(),
        output_device: "default".to_string(),
        driver: Driver::Default,
    };

    // Conditionally compile with jack if the feature is specified.
    #[cfg(all(
    any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
    ),
    feature = "jack"
    ))]
        // Manually check for flags. Can be passed through cargo with -- e.g.
        // cargo run --release --example beep --features jack -- --jack
        let host = if settings.jack {
        cpal::host_from_id(cpal::available_hosts()
            .into_iter()
            .find(|id| *id == cpal::HostId::Jack)
            .expect(
                "make sure --features jack is specified. only works on OSes where jack is available",
            )).expect("jack host unavailable")
    } else {
        cpal::default_host()
    };

    #[cfg(target_os = "windows")]
    let host = cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host");

    #[cfg(any(
    not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "windows"
    )),
    not(feature = "jack")
    ))]
        let host = cpal::default_host();

    // Find devices.
    let input_device = if settings.input_device == "default" {
        host.default_input_device()
    } else {
        host.input_devices()?
            .find(|x| x.name().map(|y| y == settings.input_device).unwrap_or(false))
    }
        .expect("failed to find input device");

    let output_device = if settings.output_device == "default" {
        host.default_output_device()
    } else {
        host.output_devices()?
            .find(|x| x.name().map(|y| y == settings.output_device).unwrap_or(false))
    }
        .expect("failed to find output device");

    println!("Using input device: \"{}\"", input_device.name()?);
    println!("Using output device: \"{}\"", output_device.name()?);

    // We'll try and use the same configuration between streams to keep it simple.
    let mut config: cpal::StreamConfig = input_device.default_input_config()?.into();
    config.buffer_size = BufferSize::Fixed(FrameCount {});

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (settings.latency / 1_000.0) * config.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * config.channels as usize;

    // The buffer to share samples
    let ring = HeapRb::<f32>::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        // The ring buffer has twice as much space as necessary to add latency here,
        // so this should never fail
        producer.try_push(0.0).unwrap()
    }

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut output_fell_behind = false;
        for &sample in data {
            if producer.try_push(sample).is_err() { // It's recommended to push entire slices, as you lock threads at every push
                output_fell_behind = true;
            }
        }
        if output_fell_behind {
            eprintln!("output stream fell behind: try increasing latency");
        }
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            *sample = match consumer.try_pop() {
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0.0
                }
            };
        }
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency");
        }
    };

    // Build streams.
    println!(
        "Attempting to build both streams with f32 samples and `{:?}`.",
        config
    );
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    let output_stream = output_device.build_output_stream(&config, output_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!(
        "Starting the input and output streams with `{}` milliseconds of latency.",
        settings.latency
    );
    input_stream.play()?;
    output_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}