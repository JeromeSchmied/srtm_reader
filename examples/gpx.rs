use gpx::{Gpx, Waypoint};
use rayon::prelude::*;
use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

fn is_00(wp: &Waypoint) -> bool {
    wp.point().x_y() == (0.0, 0.0)
}

/// (y, x)
fn needed_coords(wps: &[Waypoint]) -> BTreeSet<(i32, i32)> {
    // kinda Waypoint to (i32, i32)
    let trunc = |wp: &Waypoint| -> (i32, i32) {
        let (x, y) = wp.point().x_y();
        (y.trunc() as i32, x.trunc() as i32)
    };
    // tiles we need
    wps.iter().filter(|wp| !is_00(wp)).map(trunc).collect()
}

fn read_tiles(needs: &[(i32, i32)], elev_data_dir: impl AsRef<Path>) -> Vec<srtm_reader::Tile> {
    let elev_data_dir = elev_data_dir.as_ref();

    needs
        .par_iter()
        .map(|c| srtm_reader::get_filename(*c))
        .map(|t| elev_data_dir.join(t))
        .flat_map(|p| srtm_reader::Tile::from_file(p).inspect_err(|e| eprintln!("error: {e:#?}")))
        .collect::<Vec<_>>()
}

fn get_all_elev_data<'a>(
    needs: &'a [(i32, i32)],
    tiles: &'a [srtm_reader::Tile],
) -> HashMap<&'a (i32, i32), &'a srtm_reader::Tile> {
    assert_eq!(needs.len(), tiles.len());
    needs
        .par_iter()
        .enumerate()
        .map(|(i, coord)| (coord, tiles.get(i).unwrap()))
        .collect::<HashMap<_, _>>()
    // eprintln!("loaded elevation data: {:?}", all_elev_data.keys());
}
fn add_elev(
    wps: &mut [Waypoint],
    elev_data: &HashMap<&(i32, i32), &srtm_reader::Tile>,
    overwrite: bool,
) {
    // coord is x,y but we need y,x
    let xy_yx = |wp: &Waypoint| -> srtm_reader::Coord {
        let (x, y) = wp.point().x_y();
        (y, x).into()
    };
    wps.into_par_iter()
        .filter(|wp| (wp.elevation.is_none() || overwrite) && !is_00(wp))
        .for_each(|wp| {
            let coord = xy_yx(wp);
            let elev_data = elev_data
                .get(&coord.trunc())
                .expect("elevation data must be loaded");
            let elev = elev_data.get(coord);
            // TODO: appropriate check for invalid entry in SRTM
            if !(-400..8000).contains(&elev) {
                dbg!(coord);
                dbg!(elev_data.get(coord));
                dbg!(srtm_reader::get_filename((
                    elev_data.latitude,
                    elev_data.longitude
                )));
                dbg!(elev_data.latitude);
                dbg!(elev_data.longitude);
                dbg!(elev_data.resolution);
                dbg!(elev_data.max_height());
                dbg!(elev_data.resolution.total_len());
                dbg!(elev_data
                    .data
                    .iter()
                    .filter(|x| !(-400..4000).contains(*x))
                    .count());
            } else {
                wp.elevation = Some(elev as f64);
            }
        });
}
fn add_elev_gpx(
    gpx: &mut Gpx,
    elev_data: &HashMap<&(i32, i32), &srtm_reader::Tile>,
    overwrite: bool,
) {
    add_elev(&mut gpx.waypoints, elev_data, overwrite);
    gpx.tracks.par_iter_mut().for_each(|track| {
        track
            .segments
            .par_iter_mut()
            .for_each(|trkseg| add_elev(&mut trkseg.points, elev_data, overwrite))
    });
    gpx.routes
        .par_iter_mut()
        .for_each(|route| add_elev(&mut route.points, elev_data, overwrite));
}

fn main() {
    let args = std::env::args()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>();
    dbg!(&args);

    let gpx_contents = args
        .par_iter()
        .filter(|f| f.extension().is_some_and(|x| x == "gpx"))
        .flat_map(|p| std::fs::read_to_string(p).inspect_err(|e| eprintln!("error: {e:#?}")))
        .collect::<Vec<_>>();

    let mut gpxs = gpx_contents
        .par_iter()
        .flat_map(|j| {
            gpx::read(BufReader::new(j.as_bytes())).inspect_err(|e| eprintln!("error: {e:#?}"))
        })
        .collect::<Vec<_>>();

    let mut all_needed_coords = BTreeSet::new();
    for gpx in gpxs.iter_mut() {
        all_needed_coords.append(&mut needed_coords(&gpx.waypoints));
        for track in gpx.tracks.iter_mut() {
            for seg in track.segments.iter_mut() {
                all_needed_coords.append(&mut needed_coords(&seg.points));
            }
        }

        for route in gpx.routes.iter_mut() {
            all_needed_coords.append(&mut needed_coords(&route.points));
        }
    }
    let all_needed_coords: Vec<(i32, i32)> = all_needed_coords.iter().cloned().collect();

    let elev_data_dir = Path::new(env!("ELEV_DATA_DIR"));
    let tiles = read_tiles(&all_needed_coords, elev_data_dir);
    let elev_data = get_all_elev_data(&all_needed_coords, &tiles);

    gpxs.par_iter_mut()
        .for_each(|gpx| add_elev_gpx(gpx, &elev_data, false));

    // TODO: don't write, if nothing's changed
    gpxs.par_iter().enumerate().for_each(|(i, gpx)| {
        let path = args.get(i).unwrap();
        let fout = File::create(path).unwrap();
        // dbg!(path);
        gpx::write(gpx, fout).unwrap();
    });
}
