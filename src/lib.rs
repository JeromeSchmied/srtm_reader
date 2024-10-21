//! A performant [srtm](https://www.earthdata.nasa.gov/sensors/srtm) reader for `.hgt` files.
//!
//! # Usage
//!
//! ```rust
//! use srtm_reader::Tile;
//! use std::path::PathBuf;
//!
//! // the actual elevation of Veli Brig, 263m
//! const TRUE_ELEV: i16 = 263;
//! // the coordinates of Veli Brig, actual elevation: 263m
//! let coord = (44.4480403, 15.0733053);
//! // we get the filename, that shall include the elevation data for this `coord`
//! let filename = srtm_reader::get_filename(coord);
//! // in this case, the filename will be:
//! assert_eq!(filename, "N44E015.hgt");
//! // load the srtm tile: .hgt file
//! let tile = Tile::from_file(filename).unwrap();
//! // and finally, retrieve our elevation for Veli Brig
//! let elevation = tile.get(coord).unwrap();
//! // test with a ± 5m accuracy
//! assert!((TRUE_ELEV - 5..TRUE_ELEV + 5).contains(&elevation));
//! println!("Veli Brig:\n\t- coordinates: {coord:?}\n\t- elevation\n\t\t- actual: {TRUE_ELEV}m\n\t\t- calculated: {elevation}m");
//! ```

use byteorder::{BigEndian, ReadBytesExt};
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::Path;

/// this many rows and columns are there in a standard SRTM1 file
const EXTENT: usize = 3600;

/// the available resulutions of the SRTM data, in arc seconds
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Debug, Default)]
pub enum Resolution {
    SRTM05,
    #[default]
    SRTM1,
    SRTM3,
}

impl Resolution {
    /// the number of rows and columns in an SRTM data file of [`Resolution`]
    pub const fn extent(&self) -> usize {
        1 + match self {
            Resolution::SRTM05 => EXTENT * 2,
            Resolution::SRTM1 => EXTENT,
            Resolution::SRTM3 => EXTENT / 3,
        }
    }
    /// total file length in BigEndian, total file length in bytes is [`Resolution::total_len()`] * 2
    pub const fn total_len(&self) -> usize {
        self.extent().pow(2)
    }
}

/// the SRTM tile, which contains the actual elevation data
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tile {
    pub latitude: i32,
    pub longitude: i32,
    pub resolution: Resolution,
    pub data: Vec<i16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    ParseLatLong,
    Filesize,
    Read,
}

/// coordinates
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default)]
pub struct Coord {
    /// latitude: north-south
    pub lat: f64,
    /// longitude: east-west
    pub lon: f64,
}
impl Coord {
    pub fn new(lat: impl Into<f64>, lon: impl Into<f64>) -> Self {
        let lat = lat.into();
        let lon = lon.into();
        assert!((-90. ..=90.).contains(&lat));
        assert!((-180. ..=180.).contains(&lon));
        Self { lat, lon }
    }
    pub fn with_lat(self, lat: impl Into<f64>) -> Self {
        Self::new(lat, self.lon)
    }
    pub fn with_lon(self, lon: impl Into<f64>) -> Self {
        Self::new(self.lat, lon)
    }
    pub fn add_to_lat(self, lat: impl Into<f64>) -> Self {
        self.with_lat(self.lat + lat.into())
    }
    pub fn add_to_lon(self, lon: impl Into<f64>) -> Self {
        self.with_lon(self.lon + lon.into())
    }

    /// truncate both latitude and longitude
    pub fn trunc(&self) -> (i32, i32) {
        (self.lat.trunc() as i32, self.lon.trunc() as i32)
    }
}
impl<F1: Into<f64>, F2: Into<f64>> From<(F1, F2)> for Coord {
    fn from(value: (F1, F2)) -> Self {
        let (lat, lon) = (value.0.into(), value.1.into());
        Coord { lat, lon }
    }
}
impl Tile {
    fn empty(lat: i32, lon: i32, res: Resolution) -> Tile {
        Tile {
            latitude: lat,
            longitude: lon,
            resolution: res,
            data: Vec::new(),
        }
    }

    /// read an srtm: `.hgt` file, and create a [`Tile`] if possible
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Tile, Error> {
        let (lat, lon) = get_lat_long(&path)?;
        let res = get_resolution(&path).ok_or(Error::Filesize)?;
        // eprintln!("resolution: {res:?}");
        let file = File::open(&path).map_err(|_| Error::Read)?;
        // eprintln!("file: {file:?}");
        let reader = BufReader::new(file);
        let mut tile = Tile::empty(lat, lon, res);
        tile.data = parse(reader, tile.resolution).map_err(|e| {
            eprintln!("parse error: {e:#?}");
            Error::Read
        })?;
        Ok(tile)
    }

    /// the maximum height that this [`Tile`] contains
    pub fn max_height(&self) -> i16 {
        *self.data.iter().max().unwrap_or(&0)
    }
    pub fn min_height(&self) -> i16 {
        *self.data.iter().min().unwrap_or(&0)
    }
    /// get lower-left corner's latitude and longitude
    /// it's needed for [`Tile::get_offset()`]
    fn get_origin(&self, coord: Coord) -> Coord {
        let lat = coord.lat.trunc() + 1.; // The latitude of the lower-left corner of the tile
        let lon = coord.lon.trunc(); // The longitude of the lower-left corner of the tile
        Coord { lat, lon }
    }
    /// calculate where this `coord` is located in this [`Tile`]
    fn get_offset(&self, coord: Coord) -> (usize, usize) {
        let origin = self.get_origin(coord);
        // eprintln!("origin: ({}, {})", origin.0, origin.1);
        let extent = self.resolution.extent() as f64;

        let row = ((origin.lat - coord.lat) * extent) as usize;
        let col = ((coord.lon - origin.lon) * extent) as usize;
        (row, col)
    }

    /// get the elevation of this `coord` from this [`Tile`]
    ///
    /// # Panics
    /// If this [`Tile`] doesn't contain `coord`'s elevation
    /// *NOTE*: shouldn't happen if [`get_filename()`] was used
    pub fn get(&self, coord: impl Into<Coord>) -> Option<&i16> {
        let coord: Coord = coord.into();
        let offset = self.get_offset(coord);
        let lat = coord.lat.trunc() as i32;
        let lon = coord.lon.trunc() as i32;
        assert!(
            self.latitude <= lat,
            "hgt lat: {}, coord lat: {lat}",
            self.latitude
        );
        assert!(
            self.longitude <= lon,
            "hgt lon: {}, coord lon: {lon}",
            self.longitude
        );
        let elev = self.get_at_offset(offset.1, offset.0);
        if elev.is_some_and(|e| *e == -9999 || *e == i16::MIN) {
            eprintln!(
                "WARNING: in file {:?} {coord:?} doesn't contain a valid elevation: {elev:?}",
                get_filename((self.latitude, self.longitude))
            );
            None
        } else {
            elev
        }
    }

    fn get_at_offset(&self, x: usize, y: usize) -> Option<&i16> {
        self.data.get(self.idx(x, y))
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        assert!(
            x < self.resolution.extent() && y < self.resolution.extent(),
            "extent: {}, x: {x}, y: {y}",
            self.resolution.extent()
        );
        y * self.resolution.extent() + x
    }
}

/// guess the resolution of the file at `path`
fn get_resolution<P: AsRef<Path>>(path: P) -> Option<Resolution> {
    let from_metadata = |m: fs::Metadata| {
        let len = m.len() as usize;
        // eprintln!("len: {len}");
        if len == Resolution::SRTM05.total_len() * 2 {
            Some(Resolution::SRTM05)
        } else if len == Resolution::SRTM1.total_len() * 2 {
            Some(Resolution::SRTM1)
        } else if len == Resolution::SRTM3.total_len() * 2 {
            Some(Resolution::SRTM3)
        } else {
            eprintln!("unknown filesize: {len}");
            None
        }
    };
    fs::metadata(path)
        .inspect_err(|e| eprintln!("error: {e:#?}"))
        .ok()
        .and_then(from_metadata)
}

// FIXME: Better error handling.
fn get_lat_long<P: AsRef<Path>>(path: P) -> Result<(i32, i32), Error> {
    let stem = path.as_ref().file_stem().ok_or(Error::ParseLatLong)?;
    let desc = stem.to_str().ok_or(Error::ParseLatLong)?;
    if desc.len() != 7 {
        return Err(Error::ParseLatLong);
    }

    let get_char = |n| desc.chars().nth(n).ok_or(Error::ParseLatLong);
    let lat_sign = if get_char(0)? == 'N' { 1 } else { -1 };
    let lat: i32 = desc[1..3].parse().map_err(|_| Error::ParseLatLong)?;

    let lon_sign = if get_char(3)? == 'E' { 1 } else { -1 };
    let lon: i32 = desc[4..7].parse().map_err(|_| Error::ParseLatLong)?;
    Ok((lat_sign * lat, lon_sign * lon))
}
/// get the name of the file, which shall include this `coord`s elevation
///
/// # Usage
///
/// ```rust
/// // the `coord`inate, whe want the elevation for
/// let coord = (87.235, 10.4234423);
/// // this convenient function gives us the filename for
/// // any `coord`inate, that is `impl Into<srtm_reader::Coord>`
/// // which is true for this tuple
/// let filename = srtm_reader::get_filename(coord);
/// assert_eq!(filename, "N87E010.hgt");
/// ```
pub fn get_filename<C: Into<Coord>>(coord: C) -> String {
    let coord: Coord = coord.into();
    let lat_ch = if coord.lat >= 0. { 'N' } else { 'S' };
    let lon_ch = if coord.lon >= 0. { 'E' } else { 'W' };
    let lat = (coord.lat.trunc() as i32).abs();
    let lon = (coord.lon.trunc() as i32).abs();
    format!(
        "{lat_ch}{}{lat}{lon_ch}{}{lon}.hgt",
        if lat < 10 { "0" } else { "" },
        if lon < 10 {
            "00"
        } else if lon < 100 {
            "0"
        } else {
            ""
        }
    )
}
fn parse<R: Read>(reader: R, res: Resolution) -> io::Result<Vec<i16>> {
    let mut reader = reader;
    let mut data = Vec::new();
    // eprintln!("total size: {}", res.total_size());
    for _ in 0..res.total_len() {
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
        assert_eq!(get_lat_long(ne).unwrap(), (35, 138));

        let nw = Path::new("N35W138.hgt");
        assert_eq!(get_lat_long(nw).unwrap(), (35, -138));

        let se = Path::new("S35E138.hgt");
        assert_eq!(get_lat_long(se).unwrap(), (-35, 138));

        let sw = Path::new("S35W138.hgt");
        assert_eq!(get_lat_long(sw).unwrap(), (-35, -138));
    }
    #[test]
    fn total_file_sizes() {
        assert_eq!(103_708_802 / 2, Resolution::SRTM05.total_len());
        assert_eq!(25_934_402 / 2, Resolution::SRTM1.total_len());
        assert_eq!(2_884_802 / 2, Resolution::SRTM3.total_len());
    }
    #[test]
    fn extents() {
        assert_eq!(7201, Resolution::SRTM05.extent());
        assert_eq!(3601, Resolution::SRTM1.extent());
        assert_eq!(1201, Resolution::SRTM3.extent());
    }

    #[test]
    #[should_panic]
    fn wrong_coord_0() {
        let _ = Coord::new(-190, 42.4);
    }
    #[test]
    #[should_panic]
    fn wrong_coord_1() {
        let _ = Coord::new(180, -42.4);
    }
    #[test]
    #[should_panic]
    fn wrong_coord_2() {
        let _ = Coord::new(-90., 181.);
    }
    #[test]
    #[should_panic]
    fn wrong_coord_3() {
        let _ = Coord::new(90., -180.00001);
    }
    #[test]
    fn correct_coord_0() {
        let _ = Coord::new(-90, 180);
    }
    #[test]
    fn correct_coord_1() {
        let _ = Coord::new(90, -180);
    }
    #[test]
    fn correct_coord_2() {
        let c = Coord::new(90, -180).with_lon(-85.7);
        assert_eq!(Coord::new(90, -85.7), c);
    }
    #[test]
    fn correct_coord_3() {
        let c = Coord::new(90, -180).with_lat(0.3);
        assert_eq!(Coord::new(0.3, -180), c);
    }
    #[test]
    fn correct_coord_4() {
        let c = Coord::new(90, -180).with_lat(0.3).with_lon(83.3);
        assert_eq!(Coord::new(0.3, 83.3), c);
    }
    #[test]
    fn correct_coord_5() {
        let c: Coord = (90, -180).into();
        let c = c.with_lat(0.3).with_lon(83.3);
        assert_eq!(Coord::new(0.3, 83.3), c);
    }
    #[test]
    fn correct_coord_6() {
        let c: Coord = (90, -180).into();
        let c = c.with_lat(0.3).with_lon(83.3);
        assert_eq!(Coord::new(0.3, 83.3), c);
    }
    #[test]
    fn correct_coord_7() {
        let c: Coord = (-90, 180).into();
        let c = c.add_to_lat(0.3252).add_to_lon(-3.2);
        assert_eq!(Coord::new(-89.6748, 176.8), c);
    }
    fn coords() -> [Coord; 3] {
        [(45, 1.4).into(), (-2.3, 87).into(), (35, -7).into()]
    }
    #[test]
    fn file_names() {
        let fnames = coords()
            .iter()
            .map(|c| get_filename(*c))
            .collect::<Vec<_>>();
        assert_eq!(fnames[0], "N45E001.hgt");
        assert_eq!(fnames[1], "S02E087.hgt");
        assert_eq!(fnames[2], "N35W007.hgt");
    }
    #[test]
    fn read() {
        let coord = Coord::new(44.4480403, 15.0733053);
        let fname = get_filename(coord);
        let tile = Tile::from_file(fname).unwrap();
        assert_eq!(tile.latitude, 44);
        assert_eq!(tile.longitude, 15);
        assert_eq!(tile.resolution, Resolution::SRTM1);
        assert_eq!(tile.data.len(), Resolution::SRTM1.total_len());

        let elev = tile.get(coord);
        assert_eq!(elev, Some(&258));
    }
}
