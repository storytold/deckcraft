//! Intra prediction process (8.5.1).

use crate::tables::{D45_PRED, D63_PRED, D117_PRED, D135_PRED, D153_PRED, D207_PRED, H_PRED, TM_PRED, V_PRED};

/// Edge availability and geometry of one intra-predicted transform block.
pub struct IntraEdge {
    pub have_left: bool,
    pub have_above: bool,
    pub not_on_right: bool,
    pub tx_size: u8,
    /// Last valid column / row of the plane (in plane coordinates).
    pub max_x: usize,
    pub max_y: usize,
    pub bit_depth: u8,
}

/// Predict a `4 << tx_size` square block at (`x`, `y`) of a plane buffer. `buf` is addressed as
/// `buf[y * stride + x - x_off]` (the buffer may be a tile-column strip starting at column `x_off`).
pub fn predict(buf: &mut [u16], stride: usize, x_off: usize, x: usize, y: usize, mode: u8, e: &IntraEdge) {
    let log2 = e.tx_size as usize + 2;
    let size = 1usize << log2;
    let base = 1i32 << (e.bit_depth - 1);
    let at = |xx: usize, yy: usize| yy * stride + xx - x_off;
    // aboveRow[-1..2*size) stored at above[0..2*size+1); leftCol[0..size).
    let mut above = [0i32; 65];
    let mut left = [0i32; 32];
    if !e.have_above {
        above[1..=2 * size].fill(base - 1);
    } else {
        let row = (y - 1) * stride;
        for i in 0..size {
            above[1 + i] = buf[row + (x + i).min(e.max_x) - x_off] as i32;
        }
        if e.not_on_right && e.tx_size == 0 {
            for i in size..2 * size {
                above[1 + i] = buf[row + (x + i).min(e.max_x) - x_off] as i32;
            }
        } else {
            let v = above[size];
            above[1 + size..=2 * size].fill(v);
        }
    }
    above[0] = if e.have_above && e.have_left {
        buf[at((x - 1).min(e.max_x), y - 1)] as i32
    } else if e.have_above {
        base + 1
    } else {
        base - 1
    };
    if e.have_left {
        for i in 0..size {
            left[i] = buf[at(x - 1, (y + i).min(e.max_y))] as i32;
        }
    } else {
        left[..size].fill(base + 1);
    }
    let a = |i: isize| above[(i + 1) as usize];
    let max = (1i32 << e.bit_depth) - 1;
    let o = at(x, y);
    let mut put = |i: usize, j: usize, v: i32| buf[o + i * stride + j] = v as u16;
    let r1 = |v: i32| (v + 1) >> 1;
    let r2 = |v: i32| (v + 2) >> 2;
    match mode {
        V_PRED => {
            for i in 0..size {
                for j in 0..size {
                    put(i, j, a(j as isize));
                }
            }
        }
        H_PRED => {
            for i in 0..size {
                for j in 0..size {
                    put(i, j, left[i]);
                }
            }
        }
        D207_PRED => {
            let mut p = [[0i32; 32]; 32];
            for j in 0..size {
                p[size - 1][j] = left[size - 1];
            }
            for i in 0..size - 1 {
                p[i][0] = r1(left[i] + left[i + 1]);
            }
            for i in 0..size.saturating_sub(2) {
                p[i][1] = r2(left[i] + 2 * left[i + 1] + left[i + 2]);
            }
            p[size - 2][1] = r2(left[size - 2] + 3 * left[size - 1]);
            for j in 2..size {
                for i in (0..size - 1).rev() {
                    p[i][j] = p[i + 1][j - 2];
                }
            }
            for i in 0..size {
                for j in 0..size {
                    put(i, j, p[i][j]);
                }
            }
        }
        D45_PRED => {
            for i in 0..size {
                for j in 0..size {
                    let k = (i + j) as isize;
                    let v = if i + j + 2 < 2 * size { r2(a(k) + 2 * a(k + 1) + a(k + 2)) } else { a(2 * size as isize - 1) };
                    put(i, j, v);
                }
            }
        }
        D63_PRED => {
            for i in 0..size {
                for j in 0..size {
                    let k = (i / 2 + j) as isize;
                    let v = if i & 1 == 1 { r2(a(k) + 2 * a(k + 1) + a(k + 2)) } else { r1(a(k) + a(k + 1)) };
                    put(i, j, v);
                }
            }
        }
        D117_PRED => {
            let mut p = [[0i32; 32]; 32];
            for j in 0..size {
                p[0][j] = r1(a(j as isize - 1) + a(j as isize));
            }
            p[1][0] = r2(left[0] + 2 * a(-1) + a(0));
            for j in 1..size {
                p[1][j] = r2(a(j as isize - 2) + 2 * a(j as isize - 1) + a(j as isize));
            }
            p[2][0] = r2(a(-1) + 2 * left[0] + left[1]);
            for i in 3..size {
                p[i][0] = r2(left[i - 3] + 2 * left[i - 2] + left[i - 1]);
            }
            for i in 2..size {
                for j in 1..size {
                    p[i][j] = p[i - 2][j - 1];
                }
            }
            for i in 0..size {
                for j in 0..size {
                    put(i, j, p[i][j]);
                }
            }
        }
        D135_PRED => {
            let mut p = [[0i32; 32]; 32];
            p[0][0] = r2(left[0] + 2 * a(-1) + a(0));
            for j in 1..size {
                p[0][j] = r2(a(j as isize - 2) + 2 * a(j as isize - 1) + a(j as isize));
            }
            p[1][0] = r2(a(-1) + 2 * left[0] + left[1]);
            for i in 2..size {
                p[i][0] = r2(left[i - 2] + 2 * left[i - 1] + left[i]);
            }
            for i in 1..size {
                for j in 1..size {
                    p[i][j] = p[i - 1][j - 1];
                }
            }
            for i in 0..size {
                for j in 0..size {
                    put(i, j, p[i][j]);
                }
            }
        }
        D153_PRED => {
            let mut p = [[0i32; 32]; 32];
            p[0][0] = r1(left[0] + a(-1));
            for i in 1..size {
                p[i][0] = r1(left[i - 1] + left[i]);
            }
            p[0][1] = r2(left[0] + 2 * a(-1) + a(0));
            p[1][1] = r2(a(-1) + 2 * left[0] + left[1]);
            for i in 2..size {
                p[i][1] = r2(left[i - 2] + 2 * left[i - 1] + left[i]);
            }
            for j in 2..size {
                p[0][j] = r2(a(j as isize - 3) + 2 * a(j as isize - 2) + a(j as isize - 1));
            }
            for i in 1..size {
                for j in 2..size {
                    p[i][j] = p[i - 1][j - 2];
                }
            }
            for i in 0..size {
                for j in 0..size {
                    put(i, j, p[i][j]);
                }
            }
        }
        TM_PRED => {
            for i in 0..size {
                for j in 0..size {
                    put(i, j, (a(j as isize) + left[i] - a(-1)).clamp(0, max));
                }
            }
        }
        _ => {
            // DC_PRED
            let v = match (e.have_left, e.have_above) {
                (true, true) => {
                    let sum: i32 = left[..size].iter().sum::<i32>() + above[1..=size].iter().sum::<i32>();
                    (sum + size as i32) >> (log2 + 1)
                }
                (true, false) => (left[..size].iter().sum::<i32>() + (1 << (log2 - 1))) >> log2,
                (false, true) => (above[1..=size].iter().sum::<i32>() + (1 << (log2 - 1))) >> log2,
                (false, false) => base,
            };
            for i in 0..size {
                for j in 0..size {
                    put(i, j, v);
                }
            }
        }
    }
}
