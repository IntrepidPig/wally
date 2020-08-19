use std::{
	fs,
	io::{self, Read},
	path::PathBuf,
	process,
};

use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(name = "xcurtool", about = "A tool for working with the XCursor format")]
struct Opts {
	#[structopt(
		name = "PATH",
		help = "The path of the XCursor file to read. If omitted stdin is used"
	)]
	path: Option<PathBuf>,
}

fn main() {
	let opts = Opts::from_args();
	let buf = if let Some(path) = opts.path.as_ref() {
		match fs::read(path) {
			Ok(buf) => buf,
			Err(e) => {
				eprintln!("Failed to open file '{}': {}", path.display(), e);
				process::exit(1);
			}
		}
	} else {
		let mut buf = Vec::new();
		match io::stdin().read_to_end(&mut buf) {
			Ok(_count) => {}
			Err(e) => {
				eprintln!("Failed to read from stdin: {}", e);
				process::exit(2);
			}
		}
		buf
	};

	let xcursor = match parse(&buf) {
		Ok(xcursor) => xcursor,
		Err(e) => {
			eprintln!("Failed to parse XCursor: {:?}", e);
			process::exit(3);
		}
	};

	for (i, img) in xcursor.images.iter().enumerate() {
		let img = image::RgbaImage::from_fn(img.width, img.height, |x, y| {
			let pixel = img.pixels[(y * img.width + x) as usize];
			image::Rgba([pixel.r, pixel.g, pixel.b, pixel.a])
		});
		let filename = format!("cursor_{}.png", i);
		match img.save(&filename) {
			Ok(_) => {}
			Err(e) => {
				eprintln!("Failed to write image to file '{}': {}", filename, e);
			}
		};
	}
}

#[derive(Debug, Clone)]
pub enum ParseError {
	InvalidMagic,
	NoHeaderLength,
	NoVersion,
	ToCError,
	InvalidType,
	Unknown,
}

#[derive(Debug, Clone)]
pub struct XCursor {
	toc: Vec<ToCEntry>,
	images: Vec<Image>,
}

fn parse(buf: &[u8]) -> Result<XCursor, ParseError> {
	let raw = buf;
	let buf = match buf {
		[b'X', b'c', b'u', b'r', rest @ ..] => rest, // This backwards could signify big-endian
		_ => return Err(ParseError::InvalidMagic),
	};

	let (_header_len, buf) = take_cardinal(buf).map_err(|_| ParseError::NoHeaderLength)?;
	let (_version, buf) = take_cardinal(buf).map_err(|_| ParseError::NoVersion)?;
	let (toc, _buf) = take_toc(buf).map_err(|_| ParseError::ToCError)?;

	const COMMENT_TYPE: Cardinal = 0xfffe0001;
	const IMAGE_TYPE: Cardinal = 0xfffd0002;

	let mut images = Vec::new();

	for elem in &toc {
		match elem.r#type {
			IMAGE_TYPE => {
				images.push(parse_image(&raw[elem.position as usize..])?);
			}
			COMMENT_TYPE => {
				eprintln!("Comments not supported by this tool, ignoring...");
			}
			_ => return Err(ParseError::InvalidType),
		}
	}

	Ok(XCursor { toc, images })
}

fn take_cardinal(buf: &[u8]) -> Result<(Cardinal, &[u8]), ()> {
	let (cardinal, buf) = match buf {
		[a, b, c, d, rest @ ..] => (bytes_to_cardinal(&[*a, *b, *c, *d]), rest),
		_ => return Err(()),
	};
	Ok((cardinal, buf))
}

#[derive(Debug, Clone)]
struct Image {
	subtype: Cardinal,
	width: Cardinal,
	height: Cardinal,
	xhot: Cardinal,
	yhot: Cardinal,
	delay: Cardinal,
	pixels: Vec<Pixel>,
}

#[derive(Debug, Clone, Copy)]
struct Pixel {
	r: u8,
	g: u8,
	b: u8,
	a: u8,
}

fn parse_image(buf: &[u8]) -> Result<Image, ParseError> {
	let (_header_len, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (_type, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (subtype, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (_version, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (width, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (height, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (xhot, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let (yhot, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	dbg!(xhot, yhot);
	let (delay, buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
	let mut pixels = Vec::with_capacity((width * height) as usize);
	let mut buf = buf;
	for _ in 0..(width * height) {
		let (pixel, new_buf) = take_cardinal(buf).map_err(|_| ParseError::Unknown)?;
		buf = new_buf;
		pixels.push(Pixel {
			r: (pixel & 0x000000ff) as u8,
			g: ((pixel & 0x0000ff00) >> 8) as u8,
			b: ((pixel & 0x00ff0000) >> 16) as u8,
			a: ((pixel & 0xff000000) >> 24) as u8,
		});
	}
	Ok(Image {
		subtype,
		width,
		height,
		xhot,
		yhot,
		delay,
		pixels,
	})
}

#[derive(Debug, Clone)]
struct ToCEntry {
	r#type: Cardinal,
	subtype: Cardinal,
	position: Cardinal,
}

fn take_toc(buf: &[u8]) -> Result<(Vec<ToCEntry>, &[u8]), ()> {
	let (toc_count, buf) = take_cardinal(buf)?;
	dbg!(toc_count);
	let mut buf = buf;
	let toc_entries = (0..toc_count)
		.map(|_| {
			let (toc_entry, new_buf) = take_toc_entry(buf)?;
			buf = new_buf;
			Ok(toc_entry)
		})
		.collect::<Result<Vec<ToCEntry>, ()>>()?;
	Ok((toc_entries, buf))
}

fn take_toc_entry(buf: &[u8]) -> Result<(ToCEntry, &[u8]), ()> {
	let (r#type, buf) = take_cardinal(buf)?;
	let (subtype, buf) = take_cardinal(buf)?;
	let (position, buf) = take_cardinal(buf)?;
	let toc_entry = ToCEntry {
		r#type,
		subtype,
		position,
	};
	Ok((toc_entry, buf))
}

type Cardinal = u32;

fn bytes_to_cardinal(bytes: &[u8; 4]) -> Cardinal {
	((bytes[3] as u32) << 24) + ((bytes[2] as u32) << 16) + ((bytes[1] as u32) << 8) + bytes[0] as u32
}
