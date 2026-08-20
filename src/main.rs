use anyhow::Result;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::frame::Video;
use ffmpeg_next::media::Type;
use ffmpeg_next::software::scaling::{Context, Flags};
use image::imageops::FilterType;
use image::{DynamicImage, EncodableLayout, RgbImage};
use indicatif::ProgressIterator;

const DATA_WIDTH: usize = 584;
const DATA_HEIGHT: usize = 225;

fn img_to_bytes(img: &DynamicImage) -> Result<Vec<u8>> {
    let luma = img.to_luma8();
    let bytes = luma.as_bytes();

    let mut buf = vec![];

    for c in bytes.chunks(8) {
        let mut b = 0;
        for i in 0..u8::BITS {
            if c[i as usize] > 127 {
                b |= 1 << i;
            }
        }
        buf.push(b);
    }

    let mut data = vec![];

    let get_byte = |index: usize| -> u8 {
        let x = index % (DATA_WIDTH / 8);
        let y = index / (DATA_WIDTH / 8);
        buf[x + y * DATA_WIDTH / 8]
    };

    for i in 0..0x4029 {
        data.push(get_byte(i));
    }

    Ok(data)
}

fn main() -> Result<()> {
    ffmpeg_next::init()?;

    if let Ok(mut ictx) = ffmpeg_next::format::input("Xibalba 64.mp4") {
        let input = ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg_next::Error::StreamNotFound)?;
        let video_stream_index = input.index();

        let context_decoder =
            ffmpeg_next::codec::context::Context::from_parameters(input.parameters())?;
        let mut decoder = context_decoder.decoder().video()?;

        let mut scaler = Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGB24,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )?;

        let mut frame_index = 0;

        let mut receive_and_process_decoded_frames = |decoder: &mut ffmpeg_next::decoder::Video| -> Result<
            Vec<DynamicImage>,
            ffmpeg_next::Error,
        > {
            let mut frames = vec![];
            let mut decoded = Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                let mut rgb_frame = Video::empty();
                scaler.run(&decoded, &mut rgb_frame)?;
                frames.push(frame_to_image(&rgb_frame));
                frame_index += 1;
            }
            Ok(frames)
        };

        let mut frames = vec![];

        let bar = indicatif::ProgressBar::new_spinner();
        for (stream, packet) in ictx.packets() {
            bar.tick();
            if stream.index() == video_stream_index {
                decoder.send_packet(&packet)?;
                frames.extend(receive_and_process_decoded_frames(&mut decoder)?);
            }
        }
        decoder.send_eof()?;
        frames.extend(receive_and_process_decoded_frames(&mut decoder)?);
        bar.finish();

        let mut chunks = vec![];

        for frame in frames.into_iter().progress() {
            chunks.push(img_to_bytes(&frame)?);
        }

        let mut data = vec![];

        let mut has_flag = false;

        for chunk in chunks.into_iter().progress() {
            if chunk[0x4027] & 0x01 == 0 {
                continue;
            }

            if (chunk[0x4028] & 0x01 != 0) != has_flag {
                has_flag = chunk[0x4028] & 0x01 != 0;
                data.extend(&chunk[0..0x4000]);
            }
        }

        std::fs::write("data.bin", data)?;
    }

    Ok(())
}

fn frame_to_image(frame: &Video) -> DynamicImage {
    let mut buf = vec![];

    for i in 0..frame.height() as usize {
        let start = i * frame.stride(0);
        let len = 3 * frame.width() as usize;
        buf.extend(&frame.data(0)[start..start + len]);
    }

    DynamicImage::from(RgbImage::from_raw(frame.width(), frame.height(), buf).unwrap())
        .crop(334, 64, 1168, 900)
        .resize_exact(584, 225, FilterType::Nearest)
}
