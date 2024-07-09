use std::fs;
use std::fs::File;

use byteorder::{BigEndian, ReadBytesExt};
use std::io;
use std::io::{BufReader, Read};
use std::path::Path;

const EXTENT: u32 = 3600;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Resolution {
    SRTM05,
    SRTM1,
    SRTM3,
}

impl Resolution {
    pub const fn extent(&self) -> u32 {
        match self {
            Resolution::SRTM05 => EXTENT * 2 + 1,
            Resolution::SRTM1 => EXTENT + 1,
            Resolution::SRTM3 => EXTENT / 3 + 1,
        }
    }
    pub const fn total_size(&self) -> u32 {
        self.extent().pow(2)
    }
}

#[derive(Debug)]
pub struct Tile {
    pub latitude: i32,
    pub longitude: i32,
    pub resolution: Resolution,
    pub data: Vec<i16>,
}

#[derive(Debug)]
pub enum Error {
    ParseLatLong,
    Filesize,
    Read,
}

impl Tile {
    fn new_empty(lat: i32, lng: i32, res: Resolution) -> Tile {
        Tile {
            latitude: lat,
            longitude: lng,
            resolution: res,
            data: Vec::new(),
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Tile, Error> {
        let (lat, lng) = get_lat_long(&path)?;
        let res = get_resolution(&path).ok_or(Error::Filesize)?;
        // eprintln!("resolution: {res:?}");
        let file = File::open(&path).map_err(|_| Error::Read)?;
        // eprintln!("file: {file:?}");
        let reader = BufReader::new(file);
        let mut tile = Tile::new_empty(lat, lng, res);
        tile.data = parse(reader, tile.resolution).map_err(|e| {
            eprintln!("parse error: {e:#?}");
            Error::Read
        })?;
        Ok(tile)
    }

    pub fn max_height(&self) -> i16 {
        *(self.data.iter().max().unwrap())
    }
    fn get_origin(&self, coord: (f64, f64)) -> (f64, f64) {
        let lat = coord.0.trunc() + 1.; // The latitude of the lower-left corner of the tile
        let lon = coord.1.trunc(); // The longitude of the lower-left corner of the tile
        (lat, lon)
    }
    fn get_offset(&self, coord: (f64, f64)) -> (usize, usize) {
        let origin = self.get_origin(coord);
        eprintln!("origin: ({}, {})", origin.0, origin.1);
        let extent = self.resolution.extent() as f64;

        let row = ((origin.0 - coord.0) * extent) as usize;
        let col = ((coord.1 - origin.1) * extent) as usize;
        (row, col)
    }

    pub fn get(&self, coord: (f64, f64)) -> i16 {
        let offset = self.get_offset(coord);
        eprintln!("offset: ({}, {})", offset.1, offset.0);
        self.get_at_offset(offset.1 as u32, offset.0 as u32)
    }

    fn get_at_offset(&self, x: u32, y: u32) -> i16 {
        self.data[self.idx(x, y)]
    }

    fn idx(&self, x: u32, y: u32) -> usize {
        assert!(x < self.resolution.extent() && y < self.resolution.extent());
        (y * self.resolution.extent() + x) as usize
    }
}

fn get_resolution<P: AsRef<Path>>(path: P) -> Option<Resolution> {
    let from_metadata = |m: fs::Metadata| {
        let len = m.len();
        // eprintln!("len: {len}");
        if len == Resolution::SRTM05.total_size() as u64 * 2 {
            Some(Resolution::SRTM05)
        } else if len == Resolution::SRTM1.total_size() as u64 * 2 {
            Some(Resolution::SRTM1)
        } else if len == Resolution::SRTM3.total_size() as u64 * 2 {
            Some(Resolution::SRTM3)
        } else {
            eprintln!("unknown filesize: {}", len);
            None
        }
    };
    fs::metadata(path).ok().and_then(from_metadata)
}

// FIXME Better error handling.
fn get_lat_long<P: AsRef<Path>>(path: P) -> Result<(i32, i32), Error> {
    let stem = path.as_ref().file_stem().ok_or(Error::ParseLatLong)?;
    let desc = stem.to_str().ok_or(Error::ParseLatLong)?;
    if desc.len() != 7 {
        return Err(Error::ParseLatLong);
    }

    let get_char = |n| desc.chars().nth(n).ok_or(Error::ParseLatLong);
    let lat_sign = if get_char(0)? == 'N' { 1 } else { -1 };
    let lat: i32 = desc[1..3].parse().map_err(|_| Error::ParseLatLong)?;

    let lng_sign = if get_char(3)? == 'E' { 1 } else { -1 };
    let lng: i32 = desc[4..7].parse().map_err(|_| Error::ParseLatLong)?;
    Ok((lat_sign * lat, lng_sign * lng))
}

fn parse<R: Read>(reader: R, res: Resolution) -> io::Result<Vec<i16>> {
    let mut reader = reader;
    let mut data = Vec::new();
    // eprintln!("total size: {}", res.total_size());
    for _ in 0..res.total_size() {
        // eprint!("{i} ");
        let h = reader.read_i16::<BigEndian>()?;
        data.push(h);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_latitute_and_longitude() {
        let ne = Path::new("N35E138.hgt");
        assert_eq!(get_lat_long(&ne).unwrap(), (35, 138));

        let nw = Path::new("N35W138.hgt");
        assert_eq!(get_lat_long(&nw).unwrap(), (35, -138));

        let se = Path::new("S35E138.hgt");
        assert_eq!(get_lat_long(&se).unwrap(), (-35, 138));

        let sw = Path::new("S35W138.hgt");
        assert_eq!(get_lat_long(&sw).unwrap(), (-35, -138));
    }
    #[test]
    fn total_file_sizes() {
        assert_eq!(103_708_802, Resolution::SRTM05.total_size());
        assert_eq!(25_934_402, Resolution::SRTM1.total_size());
        assert_eq!(2_884_802, Resolution::SRTM3.total_size());
    }
    #[test]
    fn extents() {
        assert_eq!(7201, Resolution::SRTM05.extent());
        assert_eq!(3601, Resolution::SRTM1.extent());
        assert_eq!(1201, Resolution::SRTM3.extent());
    }
}
