use super::dmabuf_helpers::*;
use super::model::StartFade;
use super::readiness::{PresentationReadiness, signal_ready};
use super::shared_support::*;
use crate::dmabuf::{
    DRM_MOD_INVALID, DRM_MOD_LINEAR, FOURCC_NV12, FOURCC_XR24, format_supported, format_usable,
    preferred_tiled_modifiers, xr24_export_modifiers,
};
use crate::fill::{fill_mode, mode_uv};
use crate::shared;
use crate::timing::{Pacer, due};
use crate::{ctl, decode, vk, wayland};
use anyhow::{Context, Result};
use paper_geom::FillMode;
use std::time::Instant;

type ExportRings = Vec<Vec<vk::ExportImage>>;
type RenderTargetRings = Vec<Vec<vk::RenderTarget>>;
const DIRECT_NV12_BUFFER_BASE: usize = 1usize << (usize::BITS - 1);
const VAAPI_SURFACE_SLOTS: usize = 32;

struct SharedOutputState {
    distinct: Vec<(u32, u32)>,
    ridx: Vec<usize>,
    renderers: Vec<vk::Renderer>,
    uvs: Vec<[f32; 4]>,
    exports: ExportRings,
    rts: RenderTargetRings,
}

struct HybridNv12 {
    dims: (u32, u32),
    exports: Vec<vk::Nv12Export>,
    offset: usize,
}

struct DirectNv12Ring {
    base: usize,
    buffers: Vec<Vec<wayland_client::protocol::wl_buffer::WlBuffer>>,
    exports: Vec<vk::Nv12Export>,
}

struct VaapiDirectSlot {
    key: usize,
    descriptor: VaapiDescriptorKey,
    bi: usize,
    buffers: Vec<wayland_client::protocol::wl_buffer::WlBuffer>,
    held: Option<decode::RenderFrame>,
    pending_outputs: Vec<bool>,
}

struct VaapiDirectRing {
    base: usize,
    dims: (u32, u32),
    slots: Vec<VaapiDirectSlot>,
}

enum SlotAcquire {
    Ready(usize),
    Closed,
    Unavailable,
}

enum DirectPresentation {
    Vulkan(DirectNv12Ring),
    Vaapi(VaapiDirectRing),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VaapiPlaneKey {
    device: u64,
    inode: u64,
    object_size: usize,
    offset: usize,
    pitch: usize,
    modifier: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VaapiDescriptorKey {
    width: u32,
    height: u32,
    luma: VaapiPlaneKey,
    chroma: VaapiPlaneKey,
}

enum DirectNv12Backend {
    Presenter(Box<vk::Nv12Presenter>),
    Renderer(Box<vk::Renderer>),
}

impl DirectNv12Backend {
    fn from_env(sd: &shared::SharedDevice) -> Result<Self> {
        let selected = std::env::var("SKWD_VK_NV12_BACKEND").unwrap_or_else(|_| "presenter".into());
        match selected.as_str() {
            "presenter" => Ok(Self::Presenter(Box::new(
                vk::Nv12Presenter::new_shared((
                    sd.instance.clone(),
                    sd.phys,
                    sd.device.clone(),
                    sd.gfx_family,
                    sd.queue,
                ))
                .context("transfer-only presenter for nv12")?,
            ))),
            "renderer" => Ok(Self::Renderer(Box::new(
                vk::Renderer::new_shared_headless(
                    (
                        sd.entry.clone(),
                        sd.instance.clone(),
                        sd.phys,
                        sd.device.clone(),
                        sd.gfx_family,
                        sd.queue,
                    ),
                    64,
                    64,
                )
                .context("full headless renderer for nv12")?,
            ))),
            _ => Err(anyhow::anyhow!(
                "SKWD_VK_NV12_BACKEND must be presenter or renderer, got {selected}"
            )),
        }
    }

    fn create_nv12_export(
        &self,
        width: u32,
        height: u32,
        modifiers: &[u64],
        allow_linear: bool,
    ) -> Result<vk::Nv12Export> {
        match self {
            Self::Presenter(presenter) => {
                presenter.create_nv12_export(width, height, modifiers, allow_linear)
            }
            Self::Renderer(renderer) => {
                renderer.create_nv12_export(width, height, modifiers, allow_linear)
            }
        }
    }

    fn copy_frame(
        &mut self,
        export: &mut vk::Nv12Export,
        frame: &ffmpeg_the_third::frame::Video,
        width: u32,
        height: u32,
    ) -> Result<()> {
        match self {
            Self::Presenter(presenter) => presenter.copy_frame(export, frame, width, height),
            Self::Renderer(renderer) => {
                renderer.copy_avvk_to_nv12(export, frame, width, height)?;
                renderer.wait_render()
            }
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Presenter(_) => "transfer-only presenter",
            Self::Renderer(_) => "full headless renderer",
        }
    }
}

impl DirectNv12Ring {
    fn range(&self) -> std::ops::Range<usize> {
        self.base..self.base + self.exports.len()
    }

    fn destroy_buffers(&self) {
        for ring in &self.buffers {
            for buffer in ring {
                buffer.destroy();
            }
        }
    }
}

impl DirectPresentation {
    fn range(&self) -> std::ops::Range<usize> {
        match self {
            Self::Vulkan(ring) => ring.range(),
            Self::Vaapi(ring) => ring.base..ring.base + ring.slots.len(),
        }
    }

    fn destroy_buffers(&self) {
        match self {
            Self::Vulkan(ring) => ring.destroy_buffers(),
            Self::Vaapi(ring) => {
                for slot in &ring.slots {
                    for buffer in &slot.buffers {
                        buffer.destroy();
                    }
                }
            }
        }
    }
}

fn should_park_still(still: bool, fade_active: bool, swap_pending: bool, frames: u64) -> bool {
    still && !fade_active && !swap_pending && frames > 0
}

fn should_release_source_frame(still: bool, fading: bool) -> bool {
    !still && !fading
}

fn control_enabled_for_overlay(overlay: bool, held: bool) -> bool {
    !overlay || held
}

fn require_presentation_confirmation(confirmed: Option<bool>, label: &str) -> Result<()> {
    match confirmed {
        Some(true) => Ok(()),
        Some(false) => Err(anyhow::anyhow!(
            "compositor did not confirm presentation of {label} within 2 seconds"
        )),
        None => {
            tracing::info!(label, "presentation feedback unavailable; using commit readiness");
            Ok(())
        }
    }
}

fn confirm_presentation(
    target: &mut wayland::Target,
    checkpoint: &[u64],
    label: &str,
) -> Result<()> {
    let confirmed = target
        .wait_presentation_after(checkpoint, Instant::now() + std::time::Duration::from_secs(2))?;
    require_presentation_confirmation(confirmed, label)
}

fn build_xr_resources(
    renderers: &[vk::Renderer],
    distinct: &[(u32, u32)],
    n_exports: usize,
    exportable: bool,
    dmabuf_formats: &[(u32, u64)],
    force_linear: bool,
) -> Result<(ExportRings, RenderTargetRings)> {
    let force_linear = force_linear
        || std::env::var("SKWD_VK_XR24_LINEAR").as_deref() == Ok("1")
        || std::env::var("SKWD_VK_PATTERN").as_deref() == Ok("cpu");
    let (tiled_modifiers, linear_modifier) = xr24_export_modifiers(dmabuf_formats, force_linear);
    let exports: Vec<Vec<vk::ExportImage>> = (0..distinct.len())
        .map(|ri| {
            let (w, h) = distinct[ri];
            if !exportable {
                return (0..n_exports)
                    .map(|_| renderers[ri].create_export_image_opts(w, h, false))
                    .collect();
            }
            let mut ring = Vec::with_capacity(n_exports);
            ring.push(renderers[ri].create_xr24_export(w, h, &tiled_modifiers, linear_modifier)?);
            let selected = ring[0]
                .modifier
                .filter(|modifier| *modifier != DRM_MOD_LINEAR && *modifier != DRM_MOD_INVALID);
            for _ in 1..n_exports {
                ring.push(renderers[ri].create_xr24_export(
                    w,
                    h,
                    selected.as_slice(),
                    selected.is_none().then_some(ring[0].modifier).flatten(),
                )?);
            }
            Ok(ring)
        })
        .collect::<Result<_>>()?;
    let rts = (0..distinct.len())
        .map(|ri| {
            if exports[ri].first().is_some_and(|export| export.direct_render) {
                exports[ri].iter().map(|exp| renderers[ri].create_export_rt(exp)).collect()
            } else {
                let (w, h) = distinct[ri];
                Ok(vec![renderers[ri].create_render_target(w, h)?])
            }
        })
        .collect::<Result<_>>()?;
    Ok((exports, rts))
}

fn build_hybrid_nv12(
    renderer: &vk::Renderer,
    target: &mut wayland::Target,
    dims: (u32, u32),
    n_exports: usize,
    offset: usize,
) -> Result<(HybridNv12, Vec<Vec<wayland_client::protocol::wl_buffer::WlBuffer>>)> {
    let force_linear = std::env::var("SKWD_VK_NV12_LINEAR").as_deref() == Ok("1");
    let modifiers = if force_linear {
        Vec::new()
    } else {
        preferred_tiled_modifiers(&target.app.dmabuf_formats, FOURCC_NV12)
    };
    let linear = format_supported(&target.app.dmabuf_formats, FOURCC_NV12, 0);
    if modifiers.is_empty() && !linear {
        return Err(anyhow::anyhow!("no importable NV12 dmabuf modifier"));
    }
    let mut exports = Vec::with_capacity(n_exports);
    exports.push(renderer.create_nv12_export(dims.0, dims.1, &modifiers, linear)?);
    let first = &exports[0];
    if !target.probe_nv12_import(
        first.fd,
        dims.0,
        dims.1,
        first.plane0_offset,
        first.plane0_stride,
        first.plane1_offset,
        first.plane1_stride,
        first.modifier,
    )? {
        return Err(anyhow::anyhow!(
            "compositor rejected NV12 dmabuf import with modifier {:#018x}",
            first.modifier
        ));
    }
    let selected = [exports[0].modifier];
    for _ in 1..n_exports {
        exports.push(renderer.create_nv12_export(
            dims.0,
            dims.1,
            &selected,
            exports[0].modifier == 0,
        )?);
    }
    let buffers = (0..target.surface_count())
        .map(|si| {
            exports
                .iter()
                .enumerate()
                .map(|(bi, exp)| {
                    target.create_nv12_buffer(
                        exp.fd,
                        dims.0,
                        dims.1,
                        exp.plane0_offset,
                        exp.plane0_stride,
                        exp.plane1_offset,
                        exp.plane1_stride,
                        exp.modifier,
                        si,
                        offset + bi,
                    )
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((HybridNv12 { dims, exports, offset }, buffers))
}

#[allow(clippy::too_many_arguments)]
fn build_direct_nv12_ring(
    backend: &DirectNv12Backend,
    target: &mut wayland::Target,
    dims: (u32, u32),
    n_exports: usize,
    base: usize,
    modifiers: &[u64],
    linear: bool,
) -> Result<DirectNv12Ring> {
    let mut exports = Vec::with_capacity(n_exports);
    exports.push(backend.create_nv12_export(dims.0, dims.1, modifiers, linear)?);
    let first = &exports[0];
    if !target.probe_nv12_import(
        first.fd,
        dims.0,
        dims.1,
        first.plane0_offset,
        first.plane0_stride,
        first.plane1_offset,
        first.plane1_stride,
        first.modifier,
    )? {
        return Err(anyhow::anyhow!(
            "compositor rejected NV12 dmabuf import with modifier {:#018x}",
            first.modifier
        ));
    }
    let selected = [exports[0].modifier];
    for _ in 1..n_exports {
        exports.push(backend.create_nv12_export(
            dims.0,
            dims.1,
            &selected,
            exports[0].modifier == 0,
        )?);
    }
    let buffers = (0..target.surface_count())
        .map(|si| {
            exports
                .iter()
                .enumerate()
                .map(|(bi, exp)| {
                    target.create_nv12_buffer(
                        exp.fd,
                        dims.0,
                        dims.1,
                        exp.plane0_offset,
                        exp.plane0_stride,
                        exp.plane1_offset,
                        exp.plane1_stride,
                        exp.modifier,
                        si,
                        base + bi,
                    )
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    add_free_buffers(target, base..base + n_exports);
    Ok(DirectNv12Ring { base, buffers, exports })
}

fn vaapi_surface_key(frame: &decode::RenderFrame) -> usize {
    unsafe { (*frame.video().as_ptr()).data[3] as usize }
}

fn vaapi_plane_key(plane: &decode::PlaneDesc) -> Result<VaapiPlaneKey> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(plane.fd, stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("stat VAAPI dma-buf");
    }
    let stat = unsafe { stat.assume_init() };
    Ok(VaapiPlaneKey {
        device: stat.st_dev,
        inode: stat.st_ino,
        object_size: plane.object_size,
        offset: plane.offset,
        pitch: plane.pitch,
        modifier: plane.modifier,
    })
}

fn vaapi_descriptor_key(frame: &decode::RenderFrame) -> Result<VaapiDescriptorKey> {
    let mapped = frame.mapped_frame().context("VAAPI frame has no DRM-PRIME mapping")?;
    Ok(VaapiDescriptorKey {
        width: frame.video().width(),
        height: frame.video().height(),
        luma: vaapi_plane_key(&mapped.luma)?,
        chroma: vaapi_plane_key(&mapped.chroma)?,
    })
}

fn create_vaapi_slot(
    target: &mut wayland::Target,
    ring: &VaapiDirectRing,
    frame: &decode::RenderFrame,
) -> Result<VaapiDirectSlot> {
    if ring.slots.len() >= VAAPI_SURFACE_SLOTS {
        return Err(anyhow::anyhow!("VAAPI decoder exceeded {VAAPI_SURFACE_SLOTS} surfaces"));
    }
    let descriptor = vaapi_descriptor_key(frame)?;
    if (descriptor.width, descriptor.height) != ring.dims {
        return Err(anyhow::anyhow!(
            "VAAPI stream changed dimensions from {}x{} to {}x{}",
            ring.dims.0,
            ring.dims.1,
            descriptor.width,
            descriptor.height
        ));
    }
    let mapped = frame.mapped_frame().context("VAAPI frame has no DRM-PRIME mapping")?;
    for plane in [&mapped.luma, &mapped.chroma] {
        if !format_supported(&target.app.dmabuf_formats, FOURCC_NV12, plane.modifier) {
            return Err(anyhow::anyhow!(
                "compositor does not advertise VAAPI NV12 modifier {:#018x}",
                plane.modifier
            ));
        }
    }
    let plane0_offset = u32::try_from(mapped.luma.offset).context("VAAPI luma offset")?;
    let plane0_stride = u32::try_from(mapped.luma.pitch).context("VAAPI luma pitch")?;
    let plane1_offset = u32::try_from(mapped.chroma.offset).context("VAAPI chroma offset")?;
    let plane1_stride = u32::try_from(mapped.chroma.pitch).context("VAAPI chroma pitch")?;
    if ring.slots.is_empty()
        && !target.probe_nv12_buffer_planes(
            mapped.luma.fd,
            plane0_offset,
            plane0_stride,
            mapped.luma.modifier,
            mapped.chroma.fd,
            plane1_offset,
            plane1_stride,
            mapped.chroma.modifier,
            ring.dims.0,
            ring.dims.1,
        )?
    {
        return Err(anyhow::anyhow!("compositor rejected VAAPI NV12 plane layout"));
    }
    let bi = ring.base + ring.slots.len();
    let buffers = (0..target.surface_count())
        .map(|si| {
            target.create_nv12_buffer_planes(
                mapped.luma.fd,
                plane0_offset,
                plane0_stride,
                mapped.luma.modifier,
                mapped.chroma.fd,
                plane1_offset,
                plane1_stride,
                mapped.chroma.modifier,
                ring.dims.0,
                ring.dims.1,
                si,
                bi,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    add_free_buffers(target, bi..bi + 1);
    Ok(VaapiDirectSlot {
        key: vaapi_surface_key(frame),
        descriptor,
        bi,
        buffers,
        held: None,
        pending_outputs: vec![false; target.surface_count()],
    })
}

fn acquire_vaapi_slot(
    target: &mut wayland::Target,
    group: &[usize],
    ring: &mut VaapiDirectRing,
    frame: &decode::RenderFrame,
) -> Result<SlotAcquire> {
    frame.mapped_frame().context("VAAPI frame has no DRM-PRIME mapping")?.wait_ready()?;
    let key = vaapi_surface_key(frame);
    let descriptor = vaapi_descriptor_key(frame)?;
    let slot_index = match ring.slots.iter().position(|slot| slot.key == key) {
        Some(index) => {
            if ring.slots[index].descriptor != descriptor {
                return Err(anyhow::anyhow!("VAAPI surface backing changed during playback"));
            }
            if ring.slots[index].pending_outputs.iter().any(|pending| *pending) {
                return Err(anyhow::anyhow!("VAAPI surface reused before compositor release"));
            }
            index
        }
        None => {
            let slot = create_vaapi_slot(target, ring, frame)?;
            tracing::info!(
                "skwd-wall-vk: VAAPI direct-present surface {} uses modifier {:#018x}",
                ring.slots.len(),
                frame.mapped_frame().unwrap().luma.modifier
            );
            ring.slots.push(slot);
            ring.slots.len() - 1
        }
    };
    let bi = ring.slots[slot_index].bi;
    match target.try_free_group_buffer_in(group, bi..bi + 1)? {
        wayland::GroupBufferWait::Ready(_) => {
            ring.slots[slot_index].held = None;
            Ok(SlotAcquire::Ready(slot_index))
        }
        wayland::GroupBufferWait::Closed => Ok(SlotAcquire::Closed),
        wayland::GroupBufferWait::Unavailable => Ok(SlotAcquire::Unavailable),
    }
}

fn buffers_free(target: &wayland::Target, range: std::ops::Range<usize>) -> bool {
    target
        .app
        .surfaces
        .iter()
        .filter(|surface| !surface.closed)
        .all(|surface| range.clone().all(|bi| surface.free_buffers.contains(&bi)))
}

fn remove_free_buffers(target: &mut wayland::Target, range: std::ops::Range<usize>) {
    for surface in &mut target.app.surfaces {
        surface.free_buffers.retain(|bi| !range.contains(bi));
    }
}

fn add_free_buffers(target: &mut wayland::Target, range: std::ops::Range<usize>) {
    for surface in &mut target.app.surfaces {
        for bi in range.clone() {
            if !surface.free_buffers.contains(&bi) {
                surface.free_buffers.push(bi);
            }
        }
    }
}

fn reap_direct_nv12_rings(target: &mut wayland::Target, retired: &mut Vec<DirectPresentation>) {
    let mut ri = 0;
    while ri < retired.len() {
        release_vaapi_frames(target, &mut retired[ri]);
        let range = retired[ri].range();
        let released = match &retired[ri] {
            DirectPresentation::Vulkan(_) => buffers_free(target, range.clone()),
            DirectPresentation::Vaapi(ring) => {
                ring.slots.iter().all(|slot| slot.pending_outputs.iter().all(|pending| !pending))
            }
        };
        if !released {
            ri += 1;
            continue;
        }
        remove_free_buffers(target, range);
        let ring = retired.swap_remove(ri);
        ring.destroy_buffers();
    }
}

fn release_vaapi_frames(target: &wayland::Target, presentation: &mut DirectPresentation) {
    let DirectPresentation::Vaapi(ring) = presentation else {
        return;
    };
    for slot in &mut ring.slots {
        let released = consume_output_releases(&mut slot.pending_outputs, |si| {
            target
                .app
                .surfaces
                .get(si)
                .is_some_and(|surface| surface.free_buffers.contains(&slot.bi))
        });
        if slot.held.is_some() && released {
            slot.held = None;
        }
    }
}

fn consume_output_releases(
    pending_outputs: &mut [bool],
    mut released: impl FnMut(usize) -> bool,
) -> bool {
    for (si, pending) in pending_outputs.iter_mut().enumerate() {
        if *pending && released(si) {
            *pending = false;
        }
    }
    pending_outputs.iter().all(|pending| !pending)
}

fn release_xr_resources(
    target: &mut wayland::Target,
    buffers: &[Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    exports: &mut Vec<Vec<vk::ExportImage>>,
    rts: &mut Vec<Vec<vk::RenderTarget>>,
    n_exports: usize,
) -> bool {
    if !buffers_free(target, 0..n_exports) {
        return false;
    }
    for ring in buffers {
        for buffer in &ring[..n_exports] {
            buffer.destroy();
        }
    }
    remove_free_buffers(target, 0..n_exports);
    rts.clear();
    exports.clear();
    true
}

#[allow(clippy::too_many_arguments)]
fn restore_xr_resources(
    target: &mut wayland::Target,
    buffers: &mut [Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    renderers: &[vk::Renderer],
    distinct: &[(u32, u32)],
    render_dims: &[(u32, u32)],
    ridx: &[usize],
    exports: &mut Vec<Vec<vk::ExportImage>>,
    rts: &mut Vec<Vec<vk::RenderTarget>>,
    n_exports: usize,
    dmabuf_formats: &[(u32, u64)],
    force_linear: bool,
) -> Result<()> {
    let (new_exports, new_rts) =
        build_xr_resources(renderers, distinct, n_exports, true, dmabuf_formats, force_linear)?;
    let new_buffers = create_buffers(target, render_dims, &new_exports, ridx)?;
    for (ring, new_ring) in buffers.iter_mut().zip(new_buffers) {
        for (bi, buffer) in new_ring.into_iter().enumerate() {
            ring[bi] = buffer;
        }
    }
    *exports = new_exports;
    *rts = new_rts;
    add_free_buffers(target, 0..n_exports);
    Ok(())
}

fn rebuild_hybrid_nv12(
    target: &mut wayland::Target,
    buffers: &mut [Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    renderer: &vk::Renderer,
    hybrid: &mut Option<HybridNv12>,
    dims: (u32, u32),
    n_exports: usize,
) -> Result<bool> {
    let Some(current) = hybrid.as_ref() else {
        return Ok(false);
    };
    if current.dims == dims {
        return Ok(true);
    }
    let range = current.offset..current.offset + n_exports;
    if !buffers_free(target, range.clone()) {
        return Ok(false);
    }
    let (replacement, replacement_buffers) =
        build_hybrid_nv12(renderer, target, dims, n_exports, current.offset)?;
    for (ring, replacement_ring) in buffers.iter_mut().zip(replacement_buffers) {
        for (bi, buffer) in replacement_ring.into_iter().enumerate() {
            ring[current.offset + bi].destroy();
            ring[current.offset + bi] = buffer;
        }
    }
    *hybrid = Some(replacement);
    Ok(true)
}

fn create_hybrid_nv12(
    target: &mut wayland::Target,
    buffers: &mut [Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    renderer: &vk::Renderer,
    hybrid: &mut Option<HybridNv12>,
    dims: (u32, u32),
    n_exports: usize,
) -> Result<()> {
    let offset = n_exports;
    let (state, new_buffers) = build_hybrid_nv12(renderer, target, dims, n_exports, offset)?;
    for (ring, new_ring) in buffers.iter_mut().zip(new_buffers) {
        ring.extend(new_ring);
    }
    add_free_buffers(target, offset..offset + n_exports);
    tracing::info!(
        "skwd-wall-vk: lazy hybrid NV12 ring uses modifier {:#018x}",
        state.exports[0].modifier
    );
    *hybrid = Some(state);
    Ok(())
}

fn present_nv12(
    target: &mut wayland::Target,
    buffers: &[Vec<wayland_client::protocol::wl_buffer::WlBuffer>],
    bi: usize,
) -> Result<bool> {
    let now_ns = monotonic_ns();
    let surfaces: Vec<usize> =
        (0..buffers.len()).filter(|&si| !target.app.surfaces[si].closed).collect();
    let due = surfaces.iter().copied().any(|si| target.commit_due_at(si, now_ns));
    if due {
        for si in surfaces {
            let ring = &buffers[si];
            target.attach_at(si, &ring[bi]);
            target.request_presentation_feedback_at(si);
            target.commit_at(si);
        }
    } else {
        for si in surfaces {
            target.return_uncommitted_buffer_at(si, bi);
        }
    }
    target.flush()?;
    Ok(due)
}

fn build_shared_outputs(
    sd: &shared::SharedDevice,
    render_dims: &[(u32, u32)],
    dec: Option<&decode::AnyDecoder>,
    n_exports: usize,
    shm_present: bool,
    dmabuf_formats: &[(u32, u64)],
    force_linear: bool,
) -> Result<SharedOutputState> {
    let (distinct, ridx) = dedup_dims(render_dims);
    let renderers: Vec<vk::Renderer> = distinct
        .iter()
        .map(|&(w, h)| {
            vk::Renderer::new_shared_headless(
                (
                    sd.entry.clone(),
                    sd.instance.clone(),
                    sd.phys,
                    sd.device.clone(),
                    sd.gfx_family,
                    sd.queue,
                ),
                w,
                h,
            )
        })
        .collect::<Result<_>>()
        .context("headless renderer on shared device")?;
    let uvs = distinct
        .iter()
        .map(|&(w, h)| {
            dec.map(|dec| {
                let (vw, vh) = dec.dims();
                mode_uv(vw, vh, w, h)
            })
            .unwrap_or([1.0, 1.0, 0.0, 0.0])
        })
        .collect();
    let (exports, rts) = build_xr_resources(
        &renderers,
        &distinct,
        n_exports,
        !shm_present,
        dmabuf_formats,
        force_linear,
    )?;
    Ok(SharedOutputState { distinct, ridx, renderers, uvs, exports, rts })
}

fn probe_shared_output(target: &mut wayland::Target, state: &SharedOutputState) -> Result<bool> {
    let exp = &state.exports[0][0];
    let (width, height) = state.distinct[0];
    target.probe_dmabuf_import(exp.fd, width, height, exp.offset, exp.stride, exp.modifier)
}

pub(super) fn run_nv12(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
) -> Result<()> {
    let force_linear = std::env::var("SKWD_VK_NV12_LINEAR").as_deref() == Ok("1");
    let modifiers = if force_linear {
        Vec::new()
    } else {
        preferred_tiled_modifiers(&target.app.dmabuf_formats, FOURCC_NV12)
    };
    let linear = format_supported(&target.app.dmabuf_formats, FOURCC_NV12, 0);
    if modifiers.is_empty() && !linear {
        return Err(anyhow::anyhow!(
            "compositor does not advertise an importable NV12 dmabuf modifier"
        ));
    }
    let sd = shared::create(target.display_ptr()).context("shared device")?;
    if !sd.queue_sync {
        tracing::info!("skwd-wall-vk: FFmpeg queue callbacks unavailable, using software decode");
        return run_shared_dmabuf_with(target, video, mute, volume, None, &sd, None, &signal_ready);
    }
    if !sd.foreign_queue {
        tracing::info!("skwd-wall-vk: foreign queue ownership unavailable, using shm present");
        return run_shared_dmabuf_with(target, video, mute, volume, None, &sd, None, &signal_ready);
    }
    let n_surf = target.surface_count();
    let mut backend = DirectNv12Backend::from_env(&sd)?;
    let mut ctl = ctl::Ctl::start(video, mute, volume, true);
    target.ctl_fd = ctl.wake_fd();
    let group: Vec<usize> = (0..n_surf).collect();
    let mut path = video.to_string();
    let mut readiness = PresentationReadiness::startup();
    let mut idle_paused = false;
    let force_software_decode = shared::software_decode_required(sd.software, sd.queue_sync);
    let n_exports = 2usize;
    init_free_buffers(target, 0);
    let mut retired = Vec::<DirectPresentation>::new();
    let mut next_base = DIRECT_NV12_BUFFER_BASE;
    'source: loop {
        let decoder = decode::open_decoder(
            &path,
            Some(sd.hwdev),
            sd.video_decode,
            sd.render_node.as_deref(),
            force_software_decode,
        )?;
        if matches!(decoder, decode::AnyDecoder::Sw(_)) {
            tracing::info!(
                "skwd-wall-vk: nv12 direct-present cannot decode {path}, using dmabuf-present"
            );
            return run_shared_dmabuf_with(
                target,
                &path,
                mute,
                volume,
                None,
                &sd,
                Some(ctl),
                &signal_ready,
            );
        }
        let (vw, vh) = decoder.dims();
        let base = next_base;
        let mut presentation = match &decoder {
            decode::AnyDecoder::Vk(_) => {
                next_base = next_base
                    .checked_add(n_exports)
                    .context("NV12 buffer generation index overflow")?;
                let ring = match build_direct_nv12_ring(
                    &backend,
                    target,
                    (vw, vh),
                    n_exports,
                    base,
                    &modifiers,
                    linear,
                ) {
                    Ok(ring) => ring,
                    Err(err) if base > DIRECT_NV12_BUFFER_BASE => {
                        tracing::info!(
                            "skwd-wall-vk: replacement NV12 ring unavailable ({err:#}), using dmabuf-present"
                        );
                        return run_shared_dmabuf_with(
                            target,
                            &path,
                            mute,
                            volume,
                            None,
                            &sd,
                            Some(ctl),
                            &signal_ready,
                        );
                    }
                    Err(err) => return Err(err),
                };
                let modifier = ring.exports[0].modifier;
                tracing::info!(
                    "skwd-wall-vk: NV12 presentation ring uses modifier {modifier:#018x}"
                );
                DirectPresentation::Vulkan(ring)
            }
            decode::AnyDecoder::Vaapi(_) => {
                next_base = next_base
                    .checked_add(VAAPI_SURFACE_SLOTS)
                    .context("VAAPI buffer generation index overflow")?;
                DirectPresentation::Vaapi(VaapiDirectRing {
                    base,
                    dims: (vw, vh),
                    slots: Vec::new(),
                })
            }
            decode::AnyDecoder::Sw(_) => unreachable!(),
        };
        for si in 0..n_surf {
            target.set_viewport_cover(si, vw, vh)?;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel::<SendFrame>(1);
        decode_thread(decoder, tx, "nv12 decode thread");
        let mut pacer = Pacer::new();
        let mut source_presented = false;
        let mut last_presented: Option<ffmpeg_the_third::frame::Video> = None;
        let backend_label = match &presentation {
            DirectPresentation::Vulkan(_) => backend.label(),
            DirectPresentation::Vaapi(_) => "VAAPI source DMA-BUF",
        };
        tracing::info!(
            "skwd-wall-vk: path = NV12 direct-present ({backend_label}), video {vw}x{vh}, {n_surf} output(s)"
        );
        loop {
            target.pump()?;
            release_vaapi_frames(target, &mut presentation);
            reap_direct_nv12_rings(target, &mut retired);
            if target.app.closed {
                return Ok(());
            }
            if target.take_resized()? {
                drop(ctl);
                return wayland::Target::reexec(&wayland::ReexecSource::Video(&path));
            }
            let was_paused = ctl.paused;
            if let Some(req) = ctl.poll() {
                ctl.swap_audio(&req.to, true);
                tracing::info!("skwd-wall-vk: nv12 cut swap to {}", req.to);
                path = req.to;
                readiness.arm_swap();
                drop(rx);
                retired.push(presentation);
                continue 'source;
            }
            crate::freeze::write_last_requested(&mut ctl, last_presented.as_ref())?;
            let idle_should_pause = target.app.idle && source_presented;
            if !idle_should_pause && idle_paused {
                idle_paused = false;
                if let Some(audio) = &mut ctl.audio {
                    audio.set_pause(ctl.paused);
                }
                pacer = Pacer::new();
            }
            if idle_should_pause {
                idle_paused = true;
                if let Some(audio) = &mut ctl.audio {
                    // Idle remains authoritative if an unpause command arrives while idle.
                    audio.set_pause(true);
                }
                target.dispatch_wait_events(Instant::now() + std::time::Duration::from_secs(30))?;
                continue;
            }
            if ctl.paused && !ctl.freeze_pending() {
                target.dispatch_wait_events(Instant::now() + std::time::Duration::from_secs(30))?;
                continue;
            }
            if was_paused {
                pacer = Pacer::new();
            }
            let Ok(SendFrame(frame, pts)) = rx.recv() else {
                if source_presented || path != video {
                    return Ok(());
                }
                remove_free_buffers(target, presentation.range());
                presentation.destroy_buffers();
                return Err(anyhow::anyhow!("nv12 decode ended before the first frame"));
            };
            let now = Instant::now();
            let due = pacer.due(now, pts);
            if due > now {
                std::thread::sleep(due - now);
            }
            let acquired = match &mut presentation {
                DirectPresentation::Vulkan(ring) => {
                    let global_bi = match target.try_free_group_buffer_in(&group, ring.range())? {
                        wayland::GroupBufferWait::Ready(bi) => bi,
                        wayland::GroupBufferWait::Closed => return Ok(()),
                        wayland::GroupBufferWait::Unavailable => continue,
                    };
                    let slot = global_bi - ring.base;
                    backend
                        .copy_frame(&mut ring.exports[slot], frame.video(), vw, vh)
                        .map(|()| (global_bi, slot))
                }
                DirectPresentation::Vaapi(ring) => {
                    match acquire_vaapi_slot(target, &group, ring, &frame)? {
                        SlotAcquire::Ready(slot) => Ok((ring.slots[slot].bi, slot)),
                        SlotAcquire::Closed => return Ok(()),
                        SlotAcquire::Unavailable => continue,
                    }
                }
            };
            let (global_bi, slot) = match acquired {
                Ok(acquired) => acquired,
                Err(err) => {
                    tracing::info!(
                        "skwd-wall-vk: NV12 source presentation unavailable ({err:#}), using dmabuf-present"
                    );
                    drop(frame);
                    drop(rx);
                    retired.push(presentation);
                    return run_shared_dmabuf_with(
                        target,
                        &path,
                        mute,
                        volume,
                        None,
                        &sd,
                        Some(ctl),
                        &signal_ready,
                    );
                }
            };
            let presentation_checkpoint = target.presentation_checkpoint();
            let now_ns = monotonic_ns();
            let mut committed = false;
            for si in 0..n_surf {
                if target.app.surfaces[si].closed {
                    continue;
                }
                if target.commit_due_at(si, now_ns) {
                    let buffer = match &presentation {
                        DirectPresentation::Vulkan(ring) => &ring.buffers[si][slot],
                        DirectPresentation::Vaapi(ring) => &ring.slots[slot].buffers[si],
                    };
                    target.attach_at(si, buffer);
                    target.request_presentation_feedback_at(si);
                    target.commit_at(si);
                    if let DirectPresentation::Vaapi(ring) = &mut presentation {
                        ring.slots[slot].pending_outputs[si] = true;
                    }
                    committed = true;
                } else {
                    target.return_uncommitted_buffer_at(si, global_bi);
                }
            }
            target.flush()?;
            if committed {
                last_presented = Some(decode::retain_frame(frame.video())?);
            }
            if let DirectPresentation::Vaapi(ring) = &mut presentation
                && committed
            {
                ring.slots[slot].held = Some(frame);
            }
            source_presented |= committed;
            readiness.complete_after_presentation(
                committed,
                || {
                    confirm_presentation(target, &presentation_checkpoint, "NV12 source frame")?;
                    tracing::info!("skwd-wall-vk: NV12 source frame presented");
                    Ok::<(), anyhow::Error>(())
                },
                signal_ready,
            )?;
        }
    }
}

pub(super) fn run_shared_dmabuf(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
    start_fade: Option<StartFade>,
) -> Result<()> {
    let sd = shared::create(target.display_ptr()).context("shared device")?;
    tracing::info!("skwd-wall-vk: shared VkDevice accepted by ffmpeg");
    run_shared_dmabuf_with(target, video, mute, volume, start_fade, &sd, None, &signal_ready)
}

pub(super) fn run_shared_dmabuf_with(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
    start_fade: Option<StartFade>,
    sd: &shared::SharedDevice,
    ctl_override: Option<ctl::Ctl>,
    on_ready: &dyn Fn(),
) -> Result<()> {
    run_shared_dmabuf_with_readiness(
        target,
        video,
        mute,
        volume,
        start_fade,
        sd,
        ctl_override,
        on_ready,
        StartupReadiness::Presented,
    )
}

pub(super) fn run_shared_dmabuf_with_commit_ready(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
    start_fade: Option<StartFade>,
    sd: &shared::SharedDevice,
    ctl_override: Option<ctl::Ctl>,
    on_ready: &dyn Fn(),
) -> Result<()> {
    run_shared_dmabuf_with_readiness(
        target,
        video,
        mute,
        volume,
        start_fade,
        sd,
        ctl_override,
        on_ready,
        StartupReadiness::Committed,
    )
}

#[derive(Clone, Copy)]
enum StartupReadiness {
    Presented,
    Committed,
}

#[allow(clippy::too_many_arguments)]
fn run_shared_dmabuf_with_readiness(
    target: &mut wayland::Target,
    video: &str,
    mute: bool,
    volume: u32,
    start_fade: Option<StartFade>,
    sd: &shared::SharedDevice,
    ctl_override: Option<ctl::Ctl>,
    on_ready: &dyn Fn(),
    startup_readiness: StartupReadiness,
) -> Result<()> {
    let pattern = std::env::var("SKWD_VK_PATTERN").ok();
    let overlay = start_fade.as_ref().is_some_and(|sf| sf.overlay);
    let mut transition_held = start_fade.as_ref().is_some_and(|sf| sf.held);
    let force_software_decode = shared::software_decode_required(sd.software, sd.queue_sync);
    let mut shm_present = std::env::var("SKWD_VK_PRESENT").as_deref() == Ok("shm")
        || !format_usable(&target.app.dmabuf_formats, FOURCC_XR24)
        || sd.software
        || !sd.foreign_queue;
    if !sd.foreign_queue {
        tracing::info!("skwd-wall-vk: foreign queue ownership unavailable, using shm present");
    }
    let first_source = start_fade.as_ref().map(|sf| sf.from.clone());
    let dec = open_shared_decoder(
        &pattern,
        first_source.as_deref().unwrap_or(video),
        sd,
        force_software_decode,
    )?;
    let speed = env_speed();
    let n_surf = target.surface_count();
    let dmabuf_formats = target.app.dmabuf_formats.clone();
    let dims: Vec<(u32, u32)> = (0..n_surf).map(|si| target.size_at(si)).collect();
    let reuse_mode = std::env::var("SKWD_VK_REUSE_EXPORT").ok();
    let source_dims = dec.as_ref().map(decode::AnyDecoder::dims);
    let reuse_source_dims =
        start_fade.as_ref().and_then(|_| decode::probe_dims(video)).or(source_dims);
    let n_exports: usize = std::env::var("SKWD_VK_BUFFERS")
        .ok()
        .and_then(|val| val.parse().ok())
        .filter(|count| (2..=4).contains(count))
        .unwrap_or(2);
    let mut force_linear_xr24 = std::env::var("SKWD_VK_XR24_LINEAR").as_deref() == Ok("1");
    let reuse_supported =
        target.supports_viewporter() && !shm_present && matches!(fill_mode(), FillMode::Fill);
    let mut render_dims =
        xr_render_dims(&dims, reuse_source_dims, reuse_mode.as_deref(), reuse_supported);
    let mut reuse_export = render_dims != dims;
    let mut output_state = build_shared_outputs(
        sd,
        &render_dims,
        dec.as_ref(),
        n_exports,
        shm_present,
        &dmabuf_formats,
        force_linear_xr24,
    )?;
    if !shm_present {
        let mut accepted = probe_shared_output(target, &output_state)?;
        if !accepted && reuse_export {
            tracing::info!("skwd-wall-vk: compositor rejected shared export, using native extents");
            render_dims.clone_from(&dims);
            reuse_export = false;
            output_state = build_shared_outputs(
                sd,
                &render_dims,
                dec.as_ref(),
                n_exports,
                false,
                &dmabuf_formats,
                force_linear_xr24,
            )?;
            accepted = probe_shared_output(target, &output_state)?;
        }
        let linear_available = xr24_export_modifiers(&dmabuf_formats, true).1.is_some();
        if !accepted && !force_linear_xr24 && linear_available {
            tracing::info!("skwd-wall-vk: compositor rejected tiled dmabuf import, trying linear");
            force_linear_xr24 = true;
            output_state = build_shared_outputs(
                sd,
                &render_dims,
                dec.as_ref(),
                n_exports,
                false,
                &dmabuf_formats,
                force_linear_xr24,
            )?;
            accepted = probe_shared_output(target, &output_state)?;
        }
        if !accepted {
            tracing::info!("skwd-wall-vk: compositor rejected dmabuf import, using shm present");
            shm_present = true;
            output_state = build_shared_outputs(
                sd,
                &render_dims,
                dec.as_ref(),
                n_exports,
                true,
                &dmabuf_formats,
                force_linear_xr24,
            )?;
        }
    }
    let SharedOutputState { distinct, ridx, mut renderers, mut uvs, mut exports, mut rts } =
        output_state;
    let groups: Vec<Vec<usize>> =
        (0..distinct.len()).map(|ri| (0..n_surf).filter(|&si| ridx[si] == ri).collect()).collect();
    if exports.first().and_then(|ring| ring.first()).is_some_and(|export| export.direct_render) {
        tracing::info!("skwd-wall-vk: direct render into exports (no copy pass)");
    }
    let (mut buffers, shm) = if shm_present {
        let rbs: Vec<vk::ReadbackBuf> = distinct
            .iter()
            .enumerate()
            .map(|(ri, &(w, h))| renderers[ri].create_readback_buf(u64::from(w) * u64::from(h) * 4))
            .collect::<Result<_>>()?;
        let mut bufs = Vec::with_capacity(n_surf);
        let mut rings = Vec::with_capacity(n_surf);
        for (si, &(w, h)) in dims.iter().enumerate() {
            let (ring, ptrs, stride) = target.create_shm_ring(si, w, h, n_exports)?;
            bufs.push(ring);
            rings.push((ptrs, stride));
        }
        (bufs, Some((rings, rbs)))
    } else {
        (create_buffers(target, &render_dims, &exports, &ridx)?, None)
    };
    let hybrid_allowed = std::env::var("SKWD_VK_HYBRID_NV12").as_deref() != Ok("0")
        && pattern.is_none()
        && !shm_present
        && !sd.software
        && target.supports_viewporter()
        && matches!(fill_mode(), FillMode::Fill)
        && matches!(reuse_mode.as_deref(), None | Some("auto"));
    let hybrid_requested = hybrid_allowed
        && matches!(dec.as_ref(), Some(decode::AnyDecoder::Vk(_)))
        && source_dims
            .is_some_and(|(w, h)| w > 0 && h > 0 && w.is_multiple_of(2) && h.is_multiple_of(2));
    let mut hybrid = if hybrid_requested {
        match build_hybrid_nv12(&renderers[0], target, source_dims.unwrap(), n_exports, n_exports) {
            Ok((state, hybrid_buffers)) => {
                for (ring, hybrid_ring) in buffers.iter_mut().zip(hybrid_buffers) {
                    ring.extend(hybrid_ring);
                }
                tracing::info!(
                    "skwd-wall-vk: hybrid NV12 steady ring uses modifier {:#018x}",
                    state.exports[0].modifier
                );
                Some(state)
            }
            Err(err) => {
                tracing::info!("skwd-wall-vk: hybrid NV12 unavailable ({err:#})");
                None
            }
        }
    } else {
        None
    };
    init_free_buffers(target, n_exports * if hybrid.is_some() { 2 } else { 1 });
    for (si, &(render_w, render_h)) in render_dims.iter().enumerate() {
        if !target.app.surfaces[si].closed {
            if reuse_export {
                target.set_viewport_cover(si, render_w, render_h)?;
            } else {
                target.set_viewport_dst(si)?;
            }
        }
    }
    tracing::info!(
        "skwd-wall-vk: path = {}, {n_surf} output(s), {} export extent(s), reuse {}, speed {speed}x",
        if shm_present {
            "vk render + shm readback floor"
        } else {
            "zero-copy + dmabuf self-presentation"
        },
        distinct.len(),
        if reuse_export { reuse_mode.as_deref().unwrap_or("auto") } else { "native" }
    );

    pattern_check(&pattern, &exports)?;
    let mut still = dec.as_ref().map(|d| d.still()).unwrap_or(false);
    let (tx, rx) = std::sync::mpsc::sync_channel::<SendFrame>(1);
    let mut rx = rx;
    spawn_decode(dec, tx);
    let mut ctl = match ctl_override {
        Some(ctl) => ctl,
        None => ctl::Ctl::start_opts(
            video,
            mute,
            volume,
            pattern.is_none(),
            control_enabled_for_overlay(overlay, transition_held),
        ),
    };
    target.ctl_fd = ctl.wake_fd();
    let mut fade: Option<FadeState> = None;
    let mut active_source = video.to_string();
    let startup_from = start_fade.as_ref().map(|fade| fade.from.clone());
    let mut pending_swap: Option<PendingSwap> = None;
    let mut readiness = PresentationReadiness::startup();
    if let Some(sf) = start_fade
        && pattern.is_none()
    {
        let req = ctl::SwapReq {
            to: video.to_string(),
            duration_ms: sf.duration_ms,
            shader: sf.shader,
            properties: None,
        };
        fade = start_swap(&req, sd, force_software_decode, &distinct, &mut ctl);
        if fade.is_none() {
            tracing::info!("skwd-wall-vk: startup transition failed, keeping from-source");
        }
    }
    let mut fade_rgba_a = startup_from
        .as_deref()
        .filter(|path| !paper_control::is_video_path(path))
        .and_then(|path| match load_rgba_still(&mut renderers[0], path, &distinct) {
            Ok(source) => Some(source),
            Err(error) => {
                tracing::info!("skwd-wall-vk: RGBA transition source unavailable ({error:#})");
                None
            }
        });
    let mut fade_rgba_b = fade.as_ref().filter(|state| state.still_b).and_then(|state| {
        match load_rgba_still(&mut renderers[0], &state.path, &distinct) {
            Ok(source) => Some(source),
            Err(error) => {
                tracing::info!("skwd-wall-vk: RGBA transition target unavailable ({error:#})");
                None
            }
        }
    });
    let mut steady_rgba: Option<RgbaStillSource> = None;
    if let Some(fade) = &mut fade {
        // Image decoding/upload is setup work, not part of the authored transition duration.
        fade.t0 = Instant::now();
    }

    fn mono_ns() -> u64 {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
    }

    let mut frames: u64 = 0;
    let mut report = Instant::now();
    let hash_mode = std::env::var("SKWD_VK_HASH").is_ok();
    let mut pending: Option<(decode::RenderFrame, f64)> = None;
    let mut prev_pts = f64::NEG_INFINITY;
    let mut wall_anchor: Option<(u64, f64)> = None;
    let mut prev_commit_ns: u64 = 0;
    let mut prev_step_ns: u64 = 0;
    let mut base: Option<(u64, f64)> = None;
    let mut scheduled_q: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    let mut flip_hist: Vec<u64> = Vec::new();
    let mut chosen: Vec<Option<usize>> = vec![None; distinct.len()];
    let mut anchor = 0usize;
    let mut old_pace: Option<(Instant, f64)> = None;
    let mut queued_old: Option<(decode::RenderFrame, f64)> = None;
    let mut sw_a: Option<FrameSlot> = None;
    let mut sw_b: Option<FrameSlot> = None;
    let fade_cap = fade_fps_cap();
    let mut last_fade_render: Option<Instant> = None;
    let mut prev_content: Option<u64> = None;
    let mut skip_streak: u64 = 0;
    let mut idle_paused = false;
    let all_surfaces: Vec<usize> = (0..n_surf).collect();
    let mut xr_ready = true;
    let mut presenting_nv12 = false;
    let mut hybrid_rebuild_failed = None;
    let mut hybrid_copy_failed = false;
    let mut last_presented: Option<ffmpeg_the_third::frame::Video> = None;
    loop {
        if target.app.closed {
            return Ok(());
        }
        if target.take_resized()? {
            drop(ctl);
            return wayland::Target::reexec(&wayland::ReexecSource::Video(&active_source));
        }
        if presenting_nv12
            && xr_ready
            && release_xr_resources(target, &buffers, &mut exports, &mut rts, n_exports)
        {
            xr_ready = false;
            tracing::info!("skwd-wall-vk: steady NV12 active, released XR24 transition ring");
        }
        let was_paused = ctl.paused;
        if let Some(req) = ctl.poll()
            && pattern.is_none()
        {
            if let Some(active) = fade.take() {
                tracing::info!("skwd-wall-vk: canceling active swap before replacement");
                active.cancel();
                for rend in &mut renderers {
                    let _ = rend.retire_plane_views();
                }
                if let Some(slot) = sw_b.take() {
                    slot.destroy(&renderers[0]);
                }
                old_pace = None;
                queued_old = None;
                last_fade_render = None;
                for source in [fade_rgba_a.take(), fade_rgba_b.take(), steady_rgba.take()]
                    .into_iter()
                    .flatten()
                {
                    renderers[0].destroy_scene_texture(source.texture);
                }
            }
            if let Some(staged) = pending_swap.take() {
                staged.cancel();
            }
            pending_swap = Some(begin_swap(
                &req,
                sd.hwdev,
                sd.video_decode,
                sd.render_node.clone(),
                force_software_decode,
            ));
        }
        crate::freeze::write_last_requested(&mut ctl, last_presented.as_ref())?;
        if let Some(state) = poll_pending_swap(&mut pending_swap, &distinct, &mut ctl) {
            active_source.clone_from(&state.path);
            fade_rgba_a = steady_rgba.take();
            fade_rgba_b = state
                .still_b
                .then(|| {
                    load_rgba_still(&mut renderers[0], &state.path, &distinct).map_err(|error| {
                        tracing::info!("skwd-wall-vk: RGBA swap target unavailable ({error:#})");
                        error
                    })
                })
                .and_then(Result::ok);
            fade = Some(state);
            fade.as_mut().unwrap().t0 = Instant::now();
            readiness.arm_swap();
        }
        if fade.is_some() && !xr_ready {
            restore_xr_resources(
                target,
                &mut buffers,
                &renderers,
                &distinct,
                &render_dims,
                &ridx,
                &mut exports,
                &mut rts,
                n_exports,
                &dmabuf_formats,
                force_linear_xr24,
            )?;
            xr_ready = true;
        }
        if target.app.idle && fade.is_none() && pending_swap.is_none() && frames > 0 {
            idle_paused = true;
            if let Some(audio) = &mut ctl.audio {
                // Idle remains authoritative if an unpause command arrives while idle.
                audio.set_pause(true);
            }
            target.dispatch_wait_events(Instant::now() + std::time::Duration::from_secs(30))?;
            continue;
        }
        if idle_paused {
            idle_paused = false;
            if let Some(audio) = &mut ctl.audio {
                audio.set_pause(ctl.paused);
            }
            base = None;
            wall_anchor = None;
            scheduled_q.clear();
        }
        if ctl.paused && !ctl.freeze_pending() {
            target.dispatch_wait_events(Instant::now() + std::time::Duration::from_secs(30))?;
            continue;
        }
        if was_paused {
            base = None;
            wall_anchor = None;
            scheduled_q.clear();
        }
        if should_park_still(still, fade.is_some(), pending_swap.is_some(), frames) {
            if overlay {
                tracing::info!("skwd-wall-vk: overlay transition complete, exiting");
                return Ok(());
            }
            target.dispatch_wait_events(Instant::now() + std::time::Duration::from_secs(30))?;
            continue;
        }
        let fading = fade.is_some();
        if fading && let Some(last) = last_fade_render {
            let surface = &target.app.surfaces[anchor];
            let due = last + fade_frame_interval(fade_cap, surface.fps_limit, surface.refresh_ns);
            if Instant::now() < due {
                target.dispatch_until(due)?;
                continue;
            }
        }
        if fading {
            last_fade_render = Some(Instant::now());
        } else {
            last_fade_render = None;
        }
        if pending.is_none() {
            match next_pending(&rx, &pattern, frames) {
                Ok(frame) => pending = Some(frame),
                Err(error) if promote_transition_target(&mut fade, &mut pending) => {
                    tracing::info!(
                        "skwd-wall-vk: transition source decode ended; promoting ready target ({error:#})"
                    );
                }
                Err(error) => return Err(error),
            }
        }
        if fading {
            let (t0, mut pts0) =
                *old_pace.get_or_insert((Instant::now(), pending.as_ref().unwrap().1));
            let elapsed = t0.elapsed().as_secs_f64();
            loop {
                if queued_old.is_none() {
                    queued_old = rx.try_recv().ok().map(|SendFrame(nf, np)| (nf, np));
                }
                let Some((_, np)) = &queued_old else { break };
                let np = *np;
                if np < pts0 {
                    pts0 = np - elapsed * speed;
                    old_pace = Some((t0, pts0));
                }
                if due(elapsed, speed, np, pts0) {
                    pending = queued_old.take();
                } else {
                    break;
                }
            }
        } else {
            old_pace = None;
            queued_old = None;
        }
        let fade_mix = fade.as_mut().map(FadeState::progress);

        if !fading && !still && frames > 0 {
            let (frame, pts) = pending.as_ref().map(|(nf, np)| (nf, *np)).unwrap();
            let content = content_hash(frame);
            if content.is_some() && content == prev_content {
                let delta = (pts - prev_pts) / speed;
                let step = if delta > 0.0 { delta.min(1.0) } else { 1.0 / 24.0 };
                target.dispatch_until(Instant::now() + std::time::Duration::from_secs_f64(step))?;
                prev_pts = pts;
                pending = None;
                skip_streak += 1;
                continue;
            }
            if content.is_some() {
                prev_content = content;
                if skip_streak > 0 {
                    tracing::debug!(
                        "skwd-wall-vk: static content resumed after {skip_streak} skipped frames"
                    );
                    skip_streak = 0;
                    base = None;
                    wall_anchor = None;
                    scheduled_q.clear();
                }
            }
        }

        retarget_anchor(
            target,
            &mut anchor,
            &mut base,
            &mut wall_anchor,
            &mut scheduled_q,
            &mut flip_hist,
        );
        // A video transition normally renders through XR24 so the shader can
        // sample and combine both sources. Its exact terminal frame should,
        // however, use the same NV12 presentation path as steady playback.
        // Besides avoiding one more colour conversion, this prevents a small
        // matrix/range correction when ownership changes after the fade.
        let terminal_video_frame = fading
            && fade_mix.is_some_and(|mix| mix >= 1.0)
            && fade.as_ref().is_some_and(|state| !state.still_b);
        let (frame, pts) = if terminal_video_frame {
            let target_frame = &fade.as_ref().unwrap().cur;
            (&target_frame.0, target_frame.1)
        } else {
            pending.as_ref().map(|(nf, np)| (nf, *np)).unwrap()
        };
        let frame_dims = (frame.width(), frame.height());
        if !fading {
            let now = mono_ns();
            let commit_at_ns = commit_at(
                &mut target.app.surfaces[anchor],
                now,
                pts,
                prev_pts,
                speed,
                &mut base,
                &mut wall_anchor,
                &mut scheduled_q,
                &mut flip_hist,
                prev_commit_ns,
                prev_step_ns,
            );
            prev_step_ns = step_ns(pts, prev_pts, speed, prev_step_ns);
            prev_commit_ns = commit_at_ns;
            target.dispatch_until(
                Instant::now()
                    + std::time::Duration::from_nanos(commit_at_ns - now.min(commit_at_ns)),
            )?;
        }
        let can_use_nv12 = ((!fading && !still) || terminal_video_frame)
            && !hybrid_copy_failed
            && is_vulkan_frame(frame)
            && frame_dims.0.is_multiple_of(2)
            && frame_dims.1.is_multiple_of(2);
        if can_use_nv12
            && hybrid_allowed
            && hybrid.is_none()
            && hybrid_rebuild_failed != Some(frame_dims)
        {
            match create_hybrid_nv12(
                target,
                &mut buffers,
                &renderers[0],
                &mut hybrid,
                frame_dims,
                n_exports,
            ) {
                Ok(()) => hybrid_rebuild_failed = None,
                Err(err) => {
                    hybrid_rebuild_failed = Some(frame_dims);
                    tracing::info!("skwd-wall-vk: lazy hybrid NV12 unavailable ({err:#})");
                }
            }
        }
        if can_use_nv12
            && hybrid.as_ref().is_some_and(|state| state.dims != frame_dims)
            && hybrid_rebuild_failed != Some(frame_dims)
        {
            match rebuild_hybrid_nv12(
                target,
                &mut buffers,
                &renderers[0],
                &mut hybrid,
                frame_dims,
                n_exports,
            ) {
                Ok(true) => hybrid_rebuild_failed = None,
                Ok(false) => {}
                Err(err) => {
                    hybrid_rebuild_failed = Some(frame_dims);
                    tracing::info!("skwd-wall-vk: hybrid NV12 resize unavailable ({err:#})");
                }
            }
        }
        let mut using_nv12 =
            can_use_nv12 && hybrid.as_ref().is_some_and(|state| state.dims == frame_dims);
        if !using_nv12 && !xr_ready {
            restore_xr_resources(
                target,
                &mut buffers,
                &renderers,
                &distinct,
                &render_dims,
                &ridx,
                &mut exports,
                &mut rts,
                n_exports,
                &dmabuf_formats,
                force_linear_xr24,
            )?;
            xr_ready = true;
        }
        if using_nv12 && !presenting_nv12 {
            for si in 0..n_surf {
                if !target.app.surfaces[si].closed {
                    target.set_viewport_cover(si, frame_dims.0, frame_dims.1)?;
                }
            }
        } else if !using_nv12 && presenting_nv12 {
            for (si, &(render_w, render_h)) in render_dims.iter().enumerate() {
                if !target.app.surfaces[si].closed {
                    if reuse_export {
                        target.set_viewport_cover(si, render_w, render_h)?;
                    } else {
                        target.set_viewport_dst(si)?;
                    }
                }
            }
        }
        let nv_bi = if using_nv12 {
            let state = hybrid.as_mut().unwrap();
            let global_bi = match target
                .try_free_group_buffer_in(&all_surfaces, state.offset..state.offset + n_exports)?
            {
                wayland::GroupBufferWait::Ready(bi) => bi,
                wayland::GroupBufferWait::Closed => {
                    return Err(anyhow::anyhow!("all hybrid NV12 surfaces closed"));
                }
                wayland::GroupBufferWait::Unavailable => {
                    if should_release_source_frame(still, fading) {
                        pending = None;
                    }
                    prev_pts = pts;
                    continue;
                }
            };
            let local_bi = global_bi - state.offset;
            let copied = renderers[0].copy_avvk_to_nv12(
                &mut state.exports[local_bi],
                frame.video(),
                frame_dims.0,
                frame_dims.1,
            );
            match copied {
                Ok(()) => Some(global_bi),
                Err(err) => {
                    add_free_buffers(target, global_bi..global_bi + 1);
                    hybrid_copy_failed = true;
                    using_nv12 = false;
                    tracing::info!(
                        "skwd-wall-vk: hybrid NV12 copy unavailable, keeping XR24 path ({err:#})"
                    );
                    if !xr_ready {
                        restore_xr_resources(
                            target,
                            &mut buffers,
                            &renderers,
                            &distinct,
                            &render_dims,
                            &ridx,
                            &mut exports,
                            &mut rts,
                            n_exports,
                            &dmabuf_formats,
                            force_linear_xr24,
                        )?;
                        xr_ready = true;
                    }
                    for (si, &(render_w, render_h)) in render_dims.iter().enumerate() {
                        if !target.app.surfaces[si].closed {
                            if reuse_export {
                                target.set_viewport_cover(si, render_w, render_h)?;
                            } else {
                                target.set_viewport_dst(si)?;
                            }
                        }
                    }
                    if wait_free_buffers(target, &mut chosen, &groups, n_exports)? {
                        if should_release_source_frame(still, fading) {
                            pending = None;
                        }
                        prev_pts = pts;
                        continue;
                    }
                    None
                }
            }
        } else {
            if wait_free_buffers(target, &mut chosen, &groups, n_exports)? {
                if should_release_source_frame(still, fading) {
                    pending = None;
                }
                prev_pts = pts;
                continue;
            }
            None
        };
        dec_hash(&pattern, hash_mode, frame, frames, pts);
        if !using_nv12 && pattern.is_none() {
            if needs_frame_slot(frame) {
                ensure_frame_slot(&mut sw_a, &renderers[0], frame, pts)?;
            }
            if let (Some(fd), Some(_)) = (&fade, fade_mix)
                && needs_frame_slot(&fd.cur.0)
            {
                ensure_frame_slot(&mut sw_b, &renderers[0], &fd.cur.0, fd.cur.1)?;
            }
        }
        if !using_nv12 {
            render_surfaces(
                &mut renderers,
                &rts,
                &mut exports,
                &chosen,
                &distinct,
                &pattern,
                frame,
                &uvs,
                fade.as_ref(),
                fade_mix,
                frames,
                sw_a.as_ref(),
                sw_b.as_ref(),
                if fading {
                    fade_rgba_a.as_ref().or(steady_rgba.as_ref())
                } else {
                    steady_rgba.as_ref()
                },
                fading.then_some(fade_rgba_b.as_ref()).flatten(),
            )?;
        }

        if terminal_video_frame {
            target.request_frame_at(anchor);
        }
        let presentation_checkpoint = target.presentation_checkpoint();
        let committed = if fading && !using_nv12 {
            wait_render_all(&mut renderers, &pattern)?;
            if let Some((rings, rbs)) = &shm {
                copy_exports_to_shm(
                    &mut renderers,
                    &exports,
                    &chosen,
                    &ridx,
                    rings,
                    rbs,
                    &distinct,
                    &dims,
                )?;
            }
            target.request_frame_at(anchor);
            let committed = present_all(target, &buffers, &chosen, &ridx, n_surf)?;
            presenting_nv12 = false;
            target.wait_frame(Instant::now() + std::time::Duration::from_millis(50))?;
            committed
        } else {
            if using_nv12 {
                renderers[0].wait_render()?;
            } else {
                wait_render_all(&mut renderers, &pattern)?;
            }
            if !using_nv12 && let Some((rings, rbs)) = &shm {
                copy_exports_to_shm(
                    &mut renderers,
                    &exports,
                    &chosen,
                    &ridx,
                    rings,
                    rbs,
                    &distinct,
                    &dims,
                )?;
            }
            if let Some(bi) = nv_bi {
                let committed = present_nv12(target, &buffers, bi)?;
                presenting_nv12 = true;
                committed
            } else {
                let committed = present_all(target, &buffers, &chosen, &ridx, n_surf)?;
                presenting_nv12 = false;
                committed
            }
        };
        if terminal_video_frame && committed {
            target.wait_frame(Instant::now() + std::time::Duration::from_millis(50))?;
        }
        let ready = readiness.complete_after_presentation(
            committed,
            || match startup_readiness {
                StartupReadiness::Presented => {
                    confirm_presentation(target, &presentation_checkpoint, "rendered frame")
                }
                StartupReadiness::Committed => Ok(()),
            },
            on_ready,
        )?;
        if committed {
            last_presented = Some(decode::retain_frame(frame.video())?);
        }
        if ready && transition_held {
            ctl.set_paused(true);
            while ctl.paused && !target.app.closed {
                target.dispatch_wait_events(Instant::now() + std::time::Duration::from_secs(30))?;
                let _ = ctl.poll();
            }
            transition_held = false;
        }
        if committed && fade_mix.is_some() {
            fade.as_mut().unwrap().note_committed();
        }
        if should_release_source_frame(still, fading) {
            pending = None;
        }

        prev_pts = pts;
        let finishing_fade = committed && fade_mix.is_some_and(|mix| mix >= 1.0);
        complete_swap(
            committed.then_some(fade_mix).flatten(),
            &mut fade,
            &mut renderers,
            &mut rx,
            &mut pending,
            &mut uvs,
            &mut base,
            &mut wall_anchor,
            &mut scheduled_q,
            &mut flip_hist,
            &mut prev_pts,
            &mut prev_commit_ns,
            &mut prev_step_ns,
            &mut sw_a,
            &mut sw_b,
            &mut still,
        );
        if finishing_fade {
            if let Some(source) = fade_rgba_a.take() {
                renderers[0].destroy_scene_texture(source.texture);
            }
            if let Some(source) = steady_rgba.take() {
                renderers[0].destroy_scene_texture(source.texture);
            }
            steady_rgba = fade_rgba_b.take();
        }
        frames += 1;
        if frames == 1 && committed {
            tracing::info!(
                "skwd-wall-vk: first self-presented frame committed on {n_surf} output(s)"
            );
        }
        if frames.is_multiple_of(240) {
            report_flips(&target.app.surfaces[anchor], &mut flip_hist, frames, &mut report);
        }
    }
}

#[cfg(test)]
mod tests;
