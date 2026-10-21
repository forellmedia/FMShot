//! Zusammensetzen auf der Grafikkarte mit `wgpu` (ADR 0011): Bildschirm,
//! Webcam, Mauszeiger, Klicks und Ebenen als Texturen; Form, Rahmen,
//! Schatten und Hintergrund der Webcam im Shader. Das Ergebnis kommt als
//! BGRA zurück und geht direkt zum Encoder.

use std::sync::{Arc, mpsc};

use grabmark_core::WebcamBackground;

use crate::layout::{Placement, Scene, Source};
use crate::{FrameRef, Mask, VideoError};

/// Zeilen beim Zurücklesen müssen auf 256 Byte ausgerichtet sein.
const ROW_ALIGN: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// Abstand der Uniform-Blöcke im gemeinsamen Puffer (Mindestausrichtung).
const UNIFORM_STRIDE: u64 = 256;
/// Höchstens so viele Ebenen pro Bild (Klicks kommen selten gehäuft).
const MAX_DRAWS: usize = 64;
/// So viele Bilder dürfen gleichzeitig unterwegs sein: Während die
/// Grafikkarte eines zeichnet, wird das vorige gelesen und kodiert.
pub const IN_FLIGHT: usize = 3;

/// Wirkung auf den Hintergrund hinter der Person (siehe `compose.wgsl`).
const EFFECT_NONE: f32 = 0.0;
const EFFECT_BLUR: f32 = 1.0;
const EFFECT_REMOVE: f32 = 2.0;
/// Entfernt, aber die Webcam füllt das Bild: dunkle Fläche statt schwarz.
const EFFECT_REMOVE_FILL: f32 = 3.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerUniform {
    rect: [f32; 4],
    crop: [f32; 4],
    border_color: [f32; 4],
    frame: [f32; 2],
    radius: f32,
    border: f32,
    shadow: f32,
    opacity: f32,
    mirror: f32,
    effect: f32,
}

/// Textur einer Quelle samt Bindung; wird neu angelegt, wenn sich die
/// Grösse ändert.
struct Slot {
    texture: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
    /// Maske der Person (R8, 255 = Person); ohne Maske 1×1 weiss.
    mask: (wgpu::Texture, (u32, u32)),
}

/// Ein abgeschicktes Bild: Puffer, Auftrag und Meldung „gelesen“.
pub struct Ticket {
    /// Puffer im Ring; nach [`Readback::receive`] wieder frei.
    pub slot: usize,
    submission: wgpu::SubmissionIndex,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

/// Ring der Puffer zum Zurücklesen ([`IN_FLIGHT`] Stück). Lässt sich an
/// einen anderen Thread geben: Dort werden fertige Bilder gelesen und
/// kodiert, während hier schon das nächste entsteht.
#[derive(Clone)]
pub struct Readback {
    device: wgpu::Device,
    buffers: Arc<Vec<wgpu::Buffer>>,
    stride: usize,
}

impl Readback {
    /// Wartet auf das Bild und übergibt es (BGRA, Zeilenlänge `stride`) an
    /// `out`. Danach ist `ticket.slot` wieder frei.
    pub fn receive<R>(
        &self,
        ticket: Ticket,
        out: impl FnOnce(&[u8], usize) -> R,
    ) -> Result<R, VideoError> {
        let gpu = |e: &dyn std::fmt::Display| VideoError::Gpu(e.to_string());
        // Nur auf dieses Bild warten – das nächste darf weiterlaufen.
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(ticket.submission),
                timeout: None,
            })
            .map_err(|e| gpu(&e))?;
        ticket
            .mapped
            .recv()
            .map_err(|e| gpu(&e))?
            .map_err(|e| gpu(&e))?;
        let buffer = &self.buffers[ticket.slot];
        let result = {
            let data = buffer.get_mapped_range(..).map_err(|e| gpu(&e))?;
            out(&data, self.stride)
        };
        buffer.unmap();
        Ok(result)
    }
}

/// Setzt Bilder zusammen. Eine Instanz pro Export (Ausgabegrösse fest).
pub struct Compositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    target: wgpu::Texture,
    readback: Readback,
    size: (u32, u32),
    slots: Vec<Slot>,
    adapter: String,
    software: bool,
}

impl Compositor {
    /// Sucht eine Grafikkarte. Ohne passende Grafikkarte [`VideoError::NoGpu`].
    pub fn new(width: u32, height: u32) -> Result<Self, VideoError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| VideoError::NoGpu(e.to_string()))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("grabmark-video"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| VideoError::NoGpu(e.to_string()))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compose"),
            source: wgpu::ShaderSource::Wgsl(include_str!("compose.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<LayerUniform>() as u64,
                        ),
                    },
                    count: None,
                },
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compose"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("compose"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layers"),
            size: UNIFORM_STRIDE * MAX_DRAWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = (width as usize * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let readback = (0..IN_FLIGHT)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: (stride * height as usize) as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let readback = Readback {
            device: device.clone(),
            buffers: Arc::new(readback),
            stride,
        };
        let slots = Source::ALL
            .iter()
            .map(|_| {
                let mask = mask_texture(&device, (1, 1));
                write_mask(&queue, &mask, (1, 1), &[255]);
                Slot {
                    texture: None,
                    mask: (mask, (1, 1)),
                }
            })
            .collect();
        Ok(Self {
            device,
            queue,
            pipeline,
            layout,
            sampler,
            uniforms,
            target,
            readback,
            size: (width, height),
            slots,
            adapter: format!("{} ({:?})", info.name, info.backend),
            software: info.device_type == wgpu::DeviceType::Cpu,
        })
    }

    /// Name der Grafikkarte (für Messungen und Fehlermeldungen).
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Rechnet die Grafikkarte nur in Software (CI ohne GPU)?
    pub fn is_software(&self) -> bool {
        self.software
    }

    /// Lädt ein neues Bild einer Quelle hoch (BGRA, gerade Deckkraft). Nur
    /// nötig, wenn es sich geändert hat.
    pub fn upload(&mut self, source: Source, frame: FrameRef<'_>) {
        let slot = &mut self.slots[source.index()];
        let size = (frame.width.max(1), frame.height.max(1));
        if slot.texture.as_ref().is_none_or(|t| t.2 != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("source"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let group = bind_group(
                &self.device,
                &self.layout,
                &self.sampler,
                &self.uniforms,
                &texture,
                &slot.mask.0,
            );
            slot.texture = Some((texture, group, size));
        }
        let Some((texture, _, _)) = &slot.texture else {
            return;
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.stride as u32),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Maske der Person zum aktuellen Webcam-Bild (für „unscharf“ und
    /// „entfernt“). Darf kleiner sein als das Bild; sie wird gestreckt.
    pub fn upload_mask(&mut self, mask: &Mask) {
        let slot = &mut self.slots[Source::Webcam.index()];
        let size = (mask.width.max(1), mask.height.max(1));
        if slot.mask.1 != size {
            slot.mask = (mask_texture(&self.device, size), size);
            if let Some((texture, group, _)) = &mut slot.texture {
                *group = bind_group(
                    &self.device,
                    &self.layout,
                    &self.sampler,
                    &self.uniforms,
                    texture,
                    &slot.mask.0,
                );
            }
        }
        write_mask(&self.queue, &slot.mask.0, size, &mask.data);
    }

    /// Hat die Quelle schon ein Bild?
    pub fn has(&self, source: Source) -> bool {
        self.slots[source.index()].texture.is_some()
    }

    /// Zeichnet die Szene und übergibt das Bild (BGRA, Zeilenlänge `stride`)
    /// an `out`. Quellen ohne hochgeladenes Bild fehlen.
    pub fn render<R>(
        &mut self,
        scene: &Scene,
        out: impl FnOnce(&[u8], usize) -> R,
    ) -> Result<R, VideoError> {
        let ticket = self.submit(scene, 0)?;
        self.readback.receive(ticket, out)
    }

    /// Puffer zum Zurücklesen, für einen eigenen Thread.
    pub fn readback(&self) -> Readback {
        self.readback.clone()
    }

    /// Schickt die Szene zum Zeichnen ab, ohne auf das Ergebnis zu warten;
    /// [`Readback::receive`] holt es. `slot` (< [`IN_FLIGHT`]) muss frei
    /// sein. Später hochgeladene Bilder ändern dieses nicht mehr (die
    /// Warteschlange der Grafikkarte hält die Reihenfolge ein).
    pub fn submit(&mut self, scene: &Scene, slot: usize) -> Result<Ticket, VideoError> {
        let (w, h) = self.size;
        let view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let mut draws = Vec::new();
        for (source, placement) in scene.layers.iter().take(MAX_DRAWS) {
            let Some((_, group, _)) = &self.slots[source.index()].texture else {
                continue;
            };
            let offset = draws.len() as u64 * UNIFORM_STRIDE;
            let uniform = uniform(placement, (w as f32, h as f32));
            self.queue
                .write_buffer(&self.uniforms, offset, bytemuck::bytes_of(&uniform));
            draws.push((group, offset as u32));
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (group, offset) in draws {
                pass.set_bind_group(0, group, &[offset]);
                pass.draw(0..4, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback.buffers[slot],
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.readback.stride as u32),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let submission = self.queue.submit([encoder.finish()]);
        let (tx, mapped) = mpsc::sync_channel(1);
        self.readback.buffers[slot].map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        Ok(Ticket {
            slot,
            submission,
            mapped,
        })
    }
}

fn mask_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mask"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: MASK_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_mask(queue: &wgpu::Queue, texture: &wgpu::Texture, size: (u32, u32), data: &[u8]) {
    if data.len() < (size.0 * size.1) as usize {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.0),
            rows_per_image: Some(size.1),
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    uniforms: &wgpu::Buffer,
    texture: &wgpu::Texture,
    mask: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<LayerUniform>() as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            },
        ],
    })
}

fn effect(p: &Placement, frame: (f32, f32)) -> f32 {
    match p.background {
        WebcamBackground::Original => EFFECT_NONE,
        WebcamBackground::Blur => EFFECT_BLUR,
        WebcamBackground::Remove => {
            let fills = p.rect.width >= frame.0 - 1.0 && p.rect.height >= frame.1 - 1.0;
            if fills {
                EFFECT_REMOVE_FILL
            } else {
                EFFECT_REMOVE
            }
        }
    }
}

fn uniform(p: &Placement, frame: (f32, f32)) -> LayerUniform {
    let (bw, bc) = p
        .border
        .map(|(w, c)| {
            (
                w,
                [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    c.a as f32 / 255.0,
                ],
            )
        })
        .unwrap_or((0.0, [0.0; 4]));
    LayerUniform {
        rect: [p.rect.x, p.rect.y, p.rect.width, p.rect.height],
        crop: [p.crop.x, p.crop.y, p.crop.width, p.crop.height],
        border_color: bc,
        frame: [frame.0, frame.1],
        radius: p.radius,
        border: bw,
        shadow: p.shadow,
        opacity: p.opacity.clamp(0.0, 1.0),
        mirror: if p.mirror { 1.0 } else { 0.0 },
        effect: effect(p, frame),
    }
}
