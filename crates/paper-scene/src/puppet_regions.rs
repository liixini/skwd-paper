use super::{cstring, read_f32, read_u16, read_u32, take};
use anyhow::{Result, ensure};

const MATRIX_FLOATS: usize = 16;
const MATRIX_BYTES: usize = MATRIX_FLOATS * size_of::<f32>();
const ATTACHMENT_FIELDS: usize = 2 * size_of::<u32>();
const WEIGHT_FIELDS: usize = 3 * size_of::<u32>();
const WEIGHT_CURVE: u32 = 2;
const TRACK_FIELDS: usize = 4 * size_of::<f32>();
const EXTENDED_SKELETON: &str = "MDLS0004";
const NONFINITE_REST: &str = "nonfinite puppet rest matrix";
const DRAW_ORDER_SKELETONS: [&str; 2] = ["MDLS0003", EXTENDED_SKELETON];

#[derive(Debug, PartialEq)]
pub struct Region {
    pub size: [f32; 3],
    pub transform: [f32; MATRIX_FLOATS],
}

#[derive(Default)]
pub(super) struct Tail {
    pub regions: Vec<Region>,
    pub rest: Vec<[f32; MATRIX_FLOATS]>,
    pub draw_order: Vec<i32>,
}

pub(super) fn parse(bytes: &[u8], bones: usize, version: &str) -> Result<Tail> {
    if bytes.is_empty() {
        return Ok(Tail::default());
    }
    let mut at = 0;
    let attachments = usize::from(read_u16(bytes, &mut at)?);
    for _ in 0..attachments {
        cstring(bytes, &mut at)?;
        take(bytes, &mut at, ATTACHMENT_FIELDS + MATRIX_BYTES)?;
    }
    let mut rest = Vec::new();
    if take(bytes, &mut at, 1)?[0] != 0 {
        rest = (0..bones).map(|_| matrix(bytes, &mut at)).collect::<Result<_>>()?;
        take(bytes, &mut at, attachments * MATRIX_BYTES)?;
    }
    let weights = read_u32(bytes, &mut at)?;
    for _ in 0..weights {
        take(bytes, &mut at, WEIGHT_FIELDS)?;
        if version == EXTENDED_SKELETON && read_u32(bytes, &mut at)? & WEIGHT_CURVE != 0 {
            take(bytes, &mut at, 2 * size_of::<f32>())?;
        }
    }
    let tracks = usize::from(read_u16(bytes, &mut at)?);
    take(bytes, &mut at, tracks * size_of::<u32>())?;
    for _ in 0..tracks {
        let count = usize::from(read_u16(bytes, &mut at)?);
        take(bytes, &mut at, count * TRACK_FIELDS)?;
    }
    skip_chains(bytes, &mut at)?;
    let mut regions = Vec::new();
    if at != bytes.len() && take(bytes, &mut at, 1)?[0] != 0 {
        regions.reserve(bones);
        for _ in 0..bones {
            regions.push(region(bytes, &mut at)?);
        }
    }
    let mut draw_order = Vec::new();
    if flagged(bytes, &mut at) {
        take(bytes, &mut at, bones * size_of::<u32>())?;
    }
    if DRAW_ORDER_SKELETONS.contains(&version) && flagged(bytes, &mut at) {
        draw_order.reserve(bones);
        for _ in 0..bones {
            draw_order.push(read_u32(bytes, &mut at)?.cast_signed());
        }
    }
    Ok(Tail { regions, rest, draw_order })
}

fn flagged(bytes: &[u8], at: &mut usize) -> bool {
    let flag = bytes.get(*at).is_some_and(|byte| *byte != 0);
    *at = (*at + 1).min(bytes.len());
    flag
}

fn region(bytes: &[u8], at: &mut usize) -> Result<Region> {
    let mut region =
        Region { size: [0.0; crate::particles::SPATIAL_AXES], transform: [0.0; MATRIX_FLOATS] };
    for value in region.size.iter_mut().chain(region.transform.iter_mut()) {
        *value = read_f32(bytes, at)?;
        ensure!(value.is_finite(), "nonfinite puppet region");
    }
    Ok(region)
}

fn matrix(bytes: &[u8], at: &mut usize) -> Result<[f32; MATRIX_FLOATS]> {
    let mut matrix = [0.0; MATRIX_FLOATS];
    for value in &mut matrix {
        *value = read_f32(bytes, at)?;
        ensure!(value.is_finite(), NONFINITE_REST);
    }
    Ok(matrix)
}

fn skip_chains(bytes: &[u8], at: &mut usize) -> Result<()> {
    let chains = read_u16(bytes, at)?;
    for _ in 0..chains {
        read_u32(bytes, at)?;
        let links = read_u32(bytes, at)? as usize;
        take(bytes, at, links * size_of::<u32>())?;
        let tracks = read_u16(bytes, at)?;
        for _ in 0..tracks {
            read_u32(bytes, at)?;
            let keys = read_u16(bytes, at)?;
            for _ in 0..keys {
                take(bytes, at, TRACK_FIELDS)?;
                let values = usize::from(read_u16(bytes, at)?);
                take(bytes, at, values * size_of::<f32>())?;
            }
        }
    }
    Ok(())
}

#[expect(clippy::min_ident_chars, reason = "affine region coefficients")]
pub fn assign_regions(mesh: &super::Mesh, cells: &mut [crate::particles::image::SampleCell]) {
    use crate::particles::image::UNBOUND_BONE;
    let bind = super::world(mesh.bones.iter().map(|bone| bone.bind), &mesh.bones);
    let mut inside = vec![false; cells.len()];
    let mut ambiguous = vec![false; cells.len()];
    let mut distance = vec![0.0; cells.len()];
    for (bone, (region, bind)) in mesh.regions.iter().zip(bind).enumerate() {
        let [a, b, _, _, c, d, _, _, _, _, _, _, x, y, _, _] = region.transform;
        let transform = bind.then(super::Affine { a, b, c, d, x, y });
        let [width, height, _] = region.size;
        let first = transform.point([width, height, 0.0]);
        let second = transform.point([-width, height, 0.0]);
        let third = transform.point([-width, -height, 0.0]);
        let axes = [
            [second[0] - first[0], second[1] - first[1]],
            [third[0] - second[0], third[1] - second[1]],
        ];
        let lengths = axes.map(|axis| axis[0] * axis[0] + axis[1] * axis[1]);
        for (index, cell) in cells.iter_mut().enumerate() {
            let point = [f32::from(cell.position[0]), -f32::from(cell.position[1])];
            let projections: [f32; 2] = std::array::from_fn(|axis| {
                let origin = [first, second][axis];
                (point[0] - origin[0]) * axes[axis][0] + (point[1] - origin[1]) * axes[axis][1]
            });
            let outside = projections
                .iter()
                .zip(lengths)
                .any(|(&value, length)| value < 0.0 || value > length);
            if outside {
                if inside[index] {
                    continue;
                }
                let separation = projections
                    .into_iter()
                    .zip(lengths)
                    .fold(0.0_f32, |distance, (value, length)| {
                        distance.max(-value).max(value - length)
                    });
                if distance[index] == 0.0 || separation < distance[index] {
                    distance[index] = separation;
                    cell.bone = bone as u8;
                }
            } else {
                ambiguous[index] |= inside[index] && cell.bone != UNBOUND_BONE;
                inside[index] = true;
                cell.bone = bone as u8;
            }
        }
    }
    if !ambiguous.iter().any(|&value| value) {
        return;
    }
    let mut neighbors: Vec<_> = cells
        .iter()
        .enumerate()
        .filter(|(index, _)| !ambiguous[*index])
        .map(|(index, cell)| Neighbor {
            position: cell.position,
            bone: cell.bone,
            index,
            low: cell.position,
            high: cell.position,
        })
        .collect();
    partition(&mut neighbors, 0);
    let mut previous = Closest::default();
    for (cell, ambiguous) in cells.iter_mut().zip(ambiguous) {
        if ambiguous {
            let mut closest = previous;
            if closest.index != usize::MAX {
                let dx = f32::from(closest.position[0]) - f32::from(cell.position[0]);
                let dy = f32::from(closest.position[1]) - f32::from(cell.position[1]);
                closest.distance = dx * dx + dy * dy;
            }
            nearest(&neighbors, cell.position, 0, &mut closest);
            cell.bone = closest.bone;
            previous = closest;
        }
    }
}

struct Neighbor {
    position: [i16; 2],
    bone: u8,
    index: usize,
    low: [i16; 2],
    high: [i16; 2],
}

#[derive(Clone, Copy)]
struct Closest {
    distance: f32,
    index: usize,
    bone: u8,
    position: [i16; 2],
}

impl Default for Closest {
    fn default() -> Self {
        Self {
            distance: f32::MAX,
            index: usize::MAX,
            bone: crate::particles::image::UNBOUND_BONE,
            position: [0; 2],
        }
    }
}

fn partition(points: &mut [Neighbor], axis: usize) {
    if points.is_empty() {
        return;
    }
    let middle = points.len() / 2;
    let (left, center, right) =
        points.select_nth_unstable_by_key(middle, |point| (point.position[axis], point.index));
    partition(left, 1 - axis);
    partition(right, 1 - axis);
    for child in [left, right] {
        if !child.is_empty() {
            let root = &child[child.len() / 2];
            for axis in 0..2 {
                center.low[axis] = center.low[axis].min(root.low[axis]);
                center.high[axis] = center.high[axis].max(root.high[axis]);
            }
        }
    }
}

fn nearest(points: &[Neighbor], point: [i16; 2], axis: usize, closest: &mut Closest) {
    if points.is_empty() {
        return;
    }
    let middle = points.len() / 2;
    let candidate = &points[middle];
    let bounds = std::array::from_fn::<_, 2, _>(|axis| {
        (i32::from(point[axis]) - i32::from(candidate.high[axis]))
            .max(i32::from(candidate.low[axis]) - i32::from(point[axis]))
            .max(0) as f32
    });
    if bounds[0] * bounds[0] + bounds[1] * bounds[1] > closest.distance {
        return;
    }
    let delta = std::array::from_fn::<_, 2, _>(|axis| {
        f32::from(candidate.position[axis]) - f32::from(point[axis])
    });
    let distance = delta[0] * delta[0] + delta[1] * delta[1];
    if distance < closest.distance
        || (distance == closest.distance && candidate.index < closest.index)
    {
        *closest = Closest {
            distance,
            index: candidate.index,
            bone: candidate.bone,
            position: candidate.position,
        };
    }
    let children = [&points[..middle], &points[middle + 1..]];
    let first = usize::from(point[axis] > candidate.position[axis]);
    nearest(children[first], point, 1 - axis, closest);
    if delta[axis] * delta[axis] <= closest.distance {
        nearest(children[1 - first], point, 1 - axis, closest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_keeps_native_ties() {
        let mut points: Vec<_> = (-20..=20)
            .flat_map(|x| (-12..=12).map(move |y| [x * 7, y * 9]))
            .enumerate()
            .filter(|(index, _)| index % 3 != 0)
            .map(|(index, position)| Neighbor {
                position,
                index,
                bone: (index % 11) as u8,
                low: position,
                high: position,
            })
            .collect();
        partition(&mut points, 0);
        for x in -18..=18 {
            for y in -10..=10 {
                let point = [x * 8, y * 11];
                let mut expected = Closest::default();
                for candidate in &points {
                    let delta = std::array::from_fn::<_, 2, _>(|axis| {
                        f32::from(candidate.position[axis]) - f32::from(point[axis])
                    });
                    let distance = delta[0] * delta[0] + delta[1] * delta[1];
                    if distance < expected.distance
                        || (distance == expected.distance && candidate.index < expected.index)
                    {
                        expected = Closest {
                            distance,
                            index: candidate.index,
                            bone: candidate.bone,
                            position: candidate.position,
                        };
                    }
                }
                let mut actual = Closest::default();
                nearest(&points, point, 0, &mut actual);
                assert_eq!(
                    (actual.index, actual.bone, actual.distance),
                    (expected.index, expected.bone, expected.distance)
                );
            }
        }
    }
}
