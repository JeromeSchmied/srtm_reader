use std::io;
use std::path::PathBuf;

struct Coord(srtm_reader::Coord);
impl std::fmt::Display for Coord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.0.lat, self.0.lon)?;
        Ok(())
    }
}
impl Coord {
    fn new(lat: f64, lon: f64) -> Self {
        Coord(srtm_reader::Coord::new(lat, lon))
    }

    /// can parse format: "<LAT>,<LON>", eg: "14.43534214,32.328791"
    fn parse(str: impl AsRef<str>) -> Self {
        let coord = str
            .as_ref()
            .replace([' ', '\'', '"', 'N', 'E', 'W', 'S'], "");
        let mut coord = coord.split(',');
        let lat: f64 = coord
            .next()
            .unwrap_or_else(|| quit_help("coord parsing"))
            .parse()
            .unwrap_or_else(|_| quit_help("coord parsing"));
        let lon: f64 = coord
            .next()
            .unwrap_or_else(|| quit_help("coord parsing"))
            .parse()
            .unwrap_or_else(|_| quit_help("coord parsing"));

        Self::new(lat, lon)
    }
}

/// quit, showing help
fn quit_help(cx: &str) -> ! {
    eprintln!(
        "error: {}

Get elevation data for a coordinate from SRTM data (.hgt files).

USAGE: elev_data [OPTIONS] <ARGS>

ARGS:  <LATITUDE_FLOAT,LONGITUDE_FLOAT> 

OPTIONS:
       --elev_data_dir: <ELEVATION_DATA_DIR> or $ELEV_DATA_DIR set
       {{ --min | --max }} true: get <boundary> of file",
        if cx.is_empty() { "unknown" } else { cx }
    );
    std::process::exit(1);
}

fn get_arg<'a>(args: &'a [String], arg: &str) -> Option<&'a String> {
    args.iter()
        .enumerate()
        .find(|(_, item)| item == &arg)
        .map(|(i, _)| args.get(i + 1))?
}

fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(coord) = args.first().map(Coord::parse) else {
        quit_help("no coordinate recieved");
    };

    // eprintln!("coord: {}", coord);
    let elev_data_dir = if let Some(arg_data_dir) = get_arg(&args, "--elev_data_dir") {
        arg_data_dir
    } else if let Some(env_data_dir) = option_env!("ELEV_DATA_DIR") {
        env_data_dir
    } else {
        quit_help("no elev_data_dir got");
    };
    let elev_data_dir = PathBuf::from(elev_data_dir);
    // eprintln!("is tiff: {is_tiff}");
    // eprintln!("elev_data_dir: {}", elev_data_dir.display());
    let file_name = srtm_reader::get_filename(coord.0);
    // eprintln!("file_name: {file_path}");
    let file_path = elev_data_dir.join(file_name);
    // eprintln!("path to .hgt file: {}", file_path.display());

    let data = srtm_reader::Tile::from_file(file_path).unwrap();
    // eprintln!("resolution: {:?}", data.resolution);
    if get_arg(&args, "--max").is_some() {
        println!("max elevation in this file is {}", data.max_height());
        return Ok(());
    };
    if get_arg(&args, "--min").is_some() {
        println!("min elevation in this file is {}", data.min_height());
        return Ok(());
    };
    let elevation = data.get(coord.0);

    // eprintln!("offset: row: {row}, col: {col}");
    // let elevation = coord.get_elevation(&data);
    // coord.get_elevation(&data)

    println!("Elevation at {coord} is {elevation:?} meters");

    Ok(())
}
