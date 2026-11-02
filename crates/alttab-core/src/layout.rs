use crate::{Rect, Size, SizePreset, Style};

#[derive(Clone, Debug)]
pub struct LayoutOptions {
    pub screen: Rect,
    pub backing_scale: f64,
    pub style: Style,
    pub size: SizePreset,
    pub outer_padding: f64,
    pub spacing: f64,
    pub max_width_ratio: Option<f64>,
    pub max_height_ratio: f64,
    pub nominal_rows: Option<usize>,
    pub physical_width_mm: Option<f64>,
}
impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            screen: Rect {
                x: 0.0,
                y: 0.0,
                width: 1440.0,
                height: 900.0,
            },
            backing_scale: 1.0,
            style: Style::Thumbnails,
            size: SizePreset::Auto,
            outer_padding: 16.0,
            spacing: 8.0,
            max_width_ratio: None,
            max_height_ratio: 0.8,
            nominal_rows: None,
            physical_width_mm: None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub panel: Rect,
    pub tiles: Vec<Rect>,
    pub rows: Vec<usize>,
    pub document_size: Size,
    pub viewport_size: Size,
}
impl Layout {
    pub fn hit_test(&self, x: f64, y: f64) -> Option<usize> {
        self.tiles.iter().position(|rect| rect.contains(x, y))
    }
}

fn positive(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
fn ratio(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value.min(1.0)
    } else {
        fallback
    }
}
pub fn aspect_fit(source: Size, bounds: Size, upscale: bool) -> Size {
    let source = Size {
        width: positive(source.width),
        height: positive(source.height),
    };
    let bounds = Size {
        width: positive(bounds.width),
        height: positive(bounds.height),
    };
    if source.width == 0.0 || source.height == 0.0 || bounds.width == 0.0 || bounds.height == 0.0 {
        return Size::default();
    }
    let scale = (bounds.width / source.width).min(bounds.height / source.height);
    let scale = if upscale { scale } else { scale.min(1.0) };
    Size {
        width: source.width * scale,
        height: source.height * scale,
    }
}
pub fn pixel_size(size: Size, backing_scale: f64) -> (u32, u32) {
    let scale = if backing_scale.is_finite() && backing_scale > 0.0 {
        backing_scale
    } else {
        1.0
    };
    (
        (positive(size.width) * scale).ceil().min(u32::MAX as f64) as u32,
        (positive(size.height) * scale).ceil().min(u32::MAX as f64) as u32,
    )
}

pub fn layout(items: &[Size], options: &LayoutOptions) -> Layout {
    if options.size != SizePreset::Auto {
        return layout_at_size(items, options, options.size);
    }
    for preset in [SizePreset::Large, SizePreset::Medium, SizePreset::Small] {
        let result = layout_at_size(items, options, preset);
        if result.document_size.height <= result.viewport_size.height || preset == SizePreset::Small
        {
            return result;
        }
    }
    unreachable!()
}
fn layout_at_size(items: &[Size], options: &LayoutOptions, preset: SizePreset) -> Layout {
    let screen_width = positive(options.screen.width);
    let screen_height = positive(options.screen.height);
    let physical_ratio = options
        .physical_width_mm
        .filter(|v| v.is_finite() && *v > 0.0)
        .map_or(0.9, |mm| (600.0 / mm).clamp(0.45, 0.9));
    let width = screen_width * ratio(options.max_width_ratio.unwrap_or(physical_ratio), 0.9);
    let max_height = screen_height * ratio(options.max_height_ratio, 0.8);
    let padding = positive(options.outer_padding)
        .min(width / 2.0)
        .min(max_height / 2.0);
    let available = Size {
        width: (width - padding * 2.0).max(0.0),
        height: (max_height - padding * 2.0).max(0.0),
    };
    if items.is_empty() || available.width <= 0.0 || available.height <= 0.0 {
        return Layout {
            panel: Rect {
                x: options.screen.x,
                y: options.screen.y,
                width: 0.0,
                height: 0.0,
            },
            ..Layout::default()
        };
    }
    let spacing = positive(options.spacing)
        .min(available.width)
        .min(available.height);
    let small = preset == SizePreset::Small;
    let large = preset == SizePreset::Large;
    let default_rows = if screen_width >= screen_height {
        if small {
            5
        } else if large {
            3
        } else {
            4
        }
    } else if small {
        8
    } else if large {
        6
    } else {
        7
    };
    let rows = options.nominal_rows.unwrap_or(default_rows).clamp(1, 100);
    let nominal_height = ((available.height - spacing * (rows - 1) as f64) / rows as f64).max(1.0);
    let tile_height = match options.style {
        Style::Thumbnails => nominal_height,
        Style::Icons => {
            if small {
                94.0
            } else if large {
                174.0
            } else {
                134.0
            }
        }
        Style::Titles => {
            if small {
                34.0
            } else if large {
                46.0
            } else {
                40.0
            }
        }
    }
    .min(available.height);
    let mut tiles = Vec::with_capacity(items.len());
    let mut memberships = Vec::with_capacity(items.len());
    let mut row = 0;
    let mut x = 0.0;
    let mut y = 0.0;
    let mut row_start = 0;
    let mut content_width: f64 = 0.0;
    for item in items {
        let tile_width = match options.style {
            Style::Icons => tile_height,
            Style::Titles => available.width.min(640.0),
            Style::Thumbnails => {
                let image = aspect_fit(
                    *item,
                    Size {
                        width: available.width,
                        height: (tile_height - 30.0).max(1.0),
                    },
                    false,
                );
                image
                    .width
                    .max(tile_height * 0.6)
                    .min(tile_height * 2.5)
                    .min(available.width)
            }
        }
        .min(available.width)
        .max(f64::MIN_POSITIVE);
        if x > 0.0 && (x + tile_width > available.width || options.style == Style::Titles) {
            center_row(&mut tiles[row_start..], x - spacing, available.width);
            content_width = content_width.max(x - spacing);
            row_start = tiles.len();
            row += 1;
            x = 0.0;
            y += tile_height + spacing;
        }
        tiles.push(Rect {
            x,
            y,
            width: tile_width,
            height: tile_height,
        });
        memberships.push(row);
        x += tile_width + spacing;
    }
    center_row(&mut tiles[row_start..], x - spacing, available.width);
    content_width = content_width.max(x - spacing);
    let document_height = y + tile_height;
    let viewport_height = document_height.min(available.height);
    let panel_width = content_width + padding * 2.0;
    let horizontal_trim = (available.width - content_width) / 2.0;
    for tile in &mut tiles {
        tile.x += padding - horizontal_trim;
        tile.y += padding;
    }
    let panel_height = viewport_height + padding * 2.0;
    let screen_x = if options.screen.x.is_finite() {
        options.screen.x
    } else {
        0.0
    };
    let screen_y = if options.screen.y.is_finite() {
        options.screen.y
    } else {
        0.0
    };
    Layout {
        panel: Rect {
            x: screen_x + (screen_width - panel_width) / 2.0,
            y: screen_y + (screen_height - panel_height) / 2.0,
            width: panel_width,
            height: panel_height,
        },
        tiles,
        rows: memberships,
        document_size: Size {
            width: panel_width,
            height: document_height + padding * 2.0,
        },
        viewport_size: Size {
            width: panel_width,
            height: panel_height,
        },
    }
}
fn center_row(tiles: &mut [Rect], width: f64, available: f64) {
    let offset = (available - width).max(0.0) / 2.0;
    for tile in tiles {
        tile.x += offset;
    }
}
