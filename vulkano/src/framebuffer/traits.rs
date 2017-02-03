// Copyright (c) 2016 The vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use std::sync::Arc;

use device::Device;
use device::DeviceOwned;
use format::ClearValue;
use format::Format;
use format::FormatTy;
use framebuffer::AttachmentsList;
use framebuffer::FramebufferCreationError;
use framebuffer::FramebufferSys;
use framebuffer::RenderPass;
use framebuffer::RenderPassCreationError;
use framebuffer::RenderPassSys;
use image::Layout as ImageLayout;
use pipeline::shader::ShaderInterfaceDef;
use sync::AccessFlagBits;
use sync::PipelineStages;

use SafeDeref;
use vk;

/// Master trait for framebuffer objects. All framebuffer structs should always implement
/// this trait.
pub unsafe trait FramebufferAbstract: FramebufferRef + FramebufferRenderPassAbstract {}
unsafe impl<T> FramebufferAbstract for T where T: FramebufferRef + FramebufferRenderPassAbstract {}

/// Trait for objects that contain a Vulkan framebuffer object.
pub unsafe trait FramebufferRef {
    /// Returns an opaque struct that represents the framebuffer's internals.
    fn inner(&self) -> FramebufferSys;

    /// Returns the width, height and array layers of the framebuffer.
    fn dimensions(&self) -> [u32; 3];

    /// Returns the width of the framebuffer in pixels.
    #[inline]
    fn width(&self) -> u32 {
        self.dimensions()[0]
    }

    /// Returns the height of the framebuffer in pixels.
    #[inline]
    fn height(&self) -> u32 {
        self.dimensions()[1]
    }

    /// Returns the number of layers (or depth) of the framebuffer.
    #[inline]
    fn layers(&self) -> u32 {
        self.dimensions()[2]
    }
}

unsafe impl<T> FramebufferRef for T where T: SafeDeref, T::Target: FramebufferRef {
    #[inline]
    fn inner(&self) -> FramebufferSys {
        (**self).inner()
    }

    #[inline]
    fn dimensions(&self) -> [u32; 3] {
        (**self).dimensions()
    }
}

/// Implemented on framebuffer objects. Gives access to the render pass the framebuffer was created
/// with.
pub unsafe trait FramebufferRenderPass {
    /// Type of the render pass the framebuffer was created with.
    type RenderPass;

    /// Returns the render pass the framebuffer was created with.
    fn render_pass(&self) -> &Self::RenderPass;
}

unsafe impl<T> FramebufferRenderPass for T where T: SafeDeref, T::Target: FramebufferRenderPass {
    type RenderPass = <T::Target as FramebufferRenderPass>::RenderPass;

    #[inline]
    fn render_pass(&self) -> &Self::RenderPass {
        (**self).render_pass()
    }
}

// TODO: impl FramebufferRenderPass for &FramebufferAbstract

/// Similar to `FramebufferRenderPass`, but doesn't use any associated type and can be turned into
/// a trait object.
///
/// This trait is automatically implemented on any object that implements `FramebufferRenderPass`.
pub unsafe trait FramebufferRenderPassAbstract {
    /// Returns the render pass the framebuffer was created with.
    fn render_pass(&self) -> &RenderPassAbstract;
}

unsafe impl<T> FramebufferRenderPassAbstract for T
    where T: FramebufferRenderPass, T::RenderPass: RenderPassAbstract
{
    #[inline]
    fn render_pass(&self) -> &RenderPassAbstract {
        FramebufferRenderPass::render_pass(self) as &RenderPassAbstract
    }
}

/// Trait for objects that contain a Vulkan render pass object.
///
/// # Safety
///
/// - `inner()` and `device()` must return the same values every time.
pub unsafe trait RenderPassAbstract: DeviceOwned + RenderPassDesc {
    /// Returns an opaque object representing the render pass' internals.
    fn inner(&self) -> RenderPassSys;
}

unsafe impl<T> RenderPassAbstract for T where T: SafeDeref, T::Target: RenderPassAbstract {
    #[inline]
    fn inner(&self) -> RenderPassSys {
        (**self).inner()
    }
}

/// Trait for objects that contain the description of a render pass.
///
/// See also all the traits whose name start with `RenderPassDesc` (eg. `RenderPassDescAttachments`
/// or TODO: rename existing traits to match this). They are extensions to this trait.
///
/// # Safety
///
/// TODO: finish this section
/// - All color and depth/stencil attachments used by any given subpass must have the same number
///   of samples.
/// - The trait methods should always return the same values, unless you modify the description
///   through a mutable borrow. Once you pass the `RenderPassDesc` object to vulkano, you can still
///   access it through the `RenderPass::desc()` method that returns a shared borrow to the
///   description. It must not be possible for a shared borrow to modify the description in such a
///   way that the description changes.
/// - The provided methods shouldn't be overriden with fancy implementations. For example
///   `build_render_pass` must build a render pass from the description and not a different one.
///
pub unsafe trait RenderPassDesc {
    /// Returns the number of attachments of the render pass.
    fn num_attachments(&self) -> usize;
    /// Returns the description of an attachment.
    ///
    /// Returns `None` if `num` is superior to `num_attachments()`.
    fn attachment(&self, num: usize) -> Option<LayoutAttachmentDescription>;
    /// Returns an iterator to the list of attachments.
    #[inline]
    fn attachments(&self) -> RenderPassDescAttachments<Self> where Self: Sized {
        RenderPassDescAttachments { render_pass: self, num: 0 }
    }

    /// Returns the number of subpasses of the render pass.
    fn num_subpasses(&self) -> usize;
    /// Returns the description of a suvpass.
    ///
    /// Returns `None` if `num` is superior to `num_subpasses()`.
    fn subpass(&self, num: usize) -> Option<LayoutPassDescription>;
    /// Returns an iterator to the list of subpasses.
    #[inline]
    fn subpasses(&self) -> RenderPassDescSubpasses<Self> where Self: Sized {
        RenderPassDescSubpasses { render_pass: self, num: 0 }
    }

    /// Returns the number of dependencies of the render pass.
    fn num_dependencies(&self) -> usize;
    /// Returns the description of a dependency.
    ///
    /// Returns `None` if `num` is superior to `num_dependencies()`.
    fn dependency(&self, num: usize) -> Option<LayoutPassDependencyDescription>;
    /// Returns an iterator to the list of dependencies.
    #[inline]
    fn dependencies(&self) -> RenderPassDescDependencies<Self> where Self: Sized {
        RenderPassDescDependencies { render_pass: self, num: 0 }
    }

    /// Builds a render pass from this description.
    ///
    /// > **Note**: This function is just a shortcut for `RenderPass::new`.
    #[inline]
    fn build_render_pass(self, device: Arc<Device>)
                         -> Result<RenderPass<Self>, RenderPassCreationError>
        where Self: Sized
    {
        RenderPass::new(device, self)
    }

    /// Returns the number of color attachments of a subpass. Returns `None` if out of range.
    #[inline]
    fn num_color_attachments(&self, subpass: u32) -> Option<u32> {
        (&self).subpasses().skip(subpass as usize).next().map(|p| p.color_attachments.len() as u32)
    }

    /// Returns the number of samples of the attachments of a subpass. Returns `None` if out of
    /// range or if the subpass has no attachment. TODO: return an enum instead?
    #[inline]
    fn num_samples(&self, subpass: u32) -> Option<u32> {
        (&self).subpasses().skip(subpass as usize).next().and_then(|p| {
            // TODO: chain input attachments as well?
            p.color_attachments.iter().cloned().chain(p.depth_stencil.clone().into_iter())
                               .filter_map(|a| (&self).attachments().skip(a.0).next())
                               .next().map(|a| a.samples)
        })
    }

    /// Returns a tuple whose first element is `true` if there's a depth attachment, and whose
    /// second element is `true` if there's a stencil attachment. Returns `None` if out of range.
    #[inline]
    fn has_depth_stencil_attachment(&self, subpass: u32) -> Option<(bool, bool)> {
        (&self).subpasses().skip(subpass as usize).next().map(|p| {
            let atch_num = match p.depth_stencil {
                Some((d, _)) => d,
                None => return (false, false)
            };

            match (&self).attachments().skip(atch_num).next().unwrap().format.ty() {
                FormatTy::Depth => (true, false),
                FormatTy::Stencil => (false, true),
                FormatTy::DepthStencil => (true, true),
                _ => unreachable!()
            }
        })
    }

    /// Returns true if a subpass has a depth attachment or a depth-stencil attachment.
    #[inline]
    fn has_depth(&self, subpass: u32) -> Option<bool> {
        (&self).subpasses().skip(subpass as usize).next().map(|p| {
            let atch_num = match p.depth_stencil {
                Some((d, _)) => d,
                None => return false
            };

            match (&self).attachments().skip(atch_num).next().unwrap().format.ty() {
                FormatTy::Depth => true,
                FormatTy::Stencil => false,
                FormatTy::DepthStencil => true,
                _ => unreachable!()
            }
        })
    }

    /// Returns true if a subpass has a depth attachment or a depth-stencil attachment whose
    /// layout is not `DepthStencilReadOnlyOptimal`.
    #[inline]
    fn has_writable_depth(&self, subpass: u32) -> Option<bool> {
        (&self).subpasses().skip(subpass as usize).next().map(|p| {
            let atch_num = match p.depth_stencil {
                Some((d, l)) => {
                    if l == ImageLayout::DepthStencilReadOnlyOptimal { return false; }
                    d
                },
                None => return false
            };

            match (&self).attachments().skip(atch_num).next().unwrap().format.ty() {
                FormatTy::Depth => true,
                FormatTy::Stencil => false,
                FormatTy::DepthStencil => true,
                _ => unreachable!()
            }
        })
    }

    /// Returns true if a subpass has a stencil attachment or a depth-stencil attachment.
    #[inline]
    fn has_stencil(&self, subpass: u32) -> Option<bool> {
        (&self).subpasses().skip(subpass as usize).next().map(|p| {
            let atch_num = match p.depth_stencil {
                Some((d, _)) => d,
                None => return false
            };

            match (&self).attachments().skip(atch_num).next().unwrap().format.ty() {
                FormatTy::Depth => false,
                FormatTy::Stencil => true,
                FormatTy::DepthStencil => true,
                _ => unreachable!()
            }
        })
    }

    /// Returns true if a subpass has a stencil attachment or a depth-stencil attachment whose
    /// layout is not `DepthStencilReadOnlyOptimal`.
    #[inline]
    fn has_writable_stencil(&self, subpass: u32) -> Option<bool> {
        (&self).subpasses().skip(subpass as usize).next().map(|p| {
            let atch_num = match p.depth_stencil {
                Some((d, l)) => {
                    if l == ImageLayout::DepthStencilReadOnlyOptimal { return false; }
                    d
                },
                None => return false
            };

            match (&self).attachments().skip(atch_num).next().unwrap().format.ty() {
                FormatTy::Depth => false,
                FormatTy::Stencil => true,
                FormatTy::DepthStencil => true,
                _ => unreachable!()
            }
        })
    }
}

unsafe impl<T> RenderPassDesc for T where T: SafeDeref, T::Target: RenderPassDesc {
    #[inline]
    fn num_attachments(&self) -> usize {
        (**self).num_attachments()
    }

    #[inline]
    fn attachment(&self, num: usize) -> Option<LayoutAttachmentDescription> {
        (**self).attachment(num)
    }

    #[inline]
    fn num_subpasses(&self) -> usize {
        (**self).num_subpasses()
    }

    #[inline]
    fn subpass(&self, num: usize) -> Option<LayoutPassDescription> {
        (**self).subpass(num)
    }

    #[inline]
    fn num_dependencies(&self) -> usize {
        (**self).num_dependencies()
    }

    #[inline]
    fn dependency(&self, num: usize) -> Option<LayoutPassDependencyDescription> {
        (**self).dependency(num)
    }
}

/// Iterator to the attachments of a `RenderPassDesc`.
#[derive(Debug, Copy, Clone)]
pub struct RenderPassDescAttachments<'a, R: ?Sized + 'a> {
    render_pass: &'a R,
    num: usize,
}

impl<'a, R: ?Sized + 'a> Iterator for RenderPassDescAttachments<'a, R> where R: RenderPassDesc {
    type Item = LayoutAttachmentDescription;

    fn next(&mut self) -> Option<LayoutAttachmentDescription> {
        if self.num < self.render_pass.num_attachments() {
            let n = self.num;
            self.num += 1;
            Some(self.render_pass.attachment(n).expect("Wrong RenderPassDesc implementation"))
        } else {
            None
        }
    }
}

/// Iterator to the subpasses of a `RenderPassDesc`.
#[derive(Debug, Copy, Clone)]
pub struct RenderPassDescSubpasses<'a, R: ?Sized + 'a> {
    render_pass: &'a R,
    num: usize,
}

impl<'a, R: ?Sized + 'a> Iterator for RenderPassDescSubpasses<'a, R> where R: RenderPassDesc {
    type Item = LayoutPassDescription;

    fn next(&mut self) -> Option<LayoutPassDescription> {
        if self.num < self.render_pass.num_subpasses() {
            let n = self.num;
            self.num += 1;
            Some(self.render_pass.subpass(n).expect("Wrong RenderPassDesc implementation"))
        } else {
            None
        }
    }
}

/// Iterator to the subpass dependencies of a `RenderPassDesc`.
#[derive(Debug, Copy, Clone)]
pub struct RenderPassDescDependencies<'a, R: ?Sized + 'a> {
    render_pass: &'a R,
    num: usize,
}

impl<'a, R: ?Sized + 'a> Iterator for RenderPassDescDependencies<'a, R> where R: RenderPassDesc {
    type Item = LayoutPassDependencyDescription;

    fn next(&mut self) -> Option<LayoutPassDependencyDescription> {
        if self.num < self.render_pass.num_dependencies() {
            let n = self.num;
            self.num += 1;
            Some(self.render_pass.dependency(n).expect("Wrong RenderPassDesc implementation"))
        } else {
            None
        }
    }
}

/// Extension trait for `RenderPassDesc`. Defines which types are allowed as an attachments list.
///
/// When the user creates a framebuffer, they need to pass a render pass object and a list of
/// attachments. In order for it to work, the `RenderPassDesc` object of the render pass must
/// implement `RenderPassDescAttachmentsList<A>` where `A` is the type of the list of attachments.
///
/// # Safety
///
/// This trait is unsafe because it's the job of the implementation to check whether the
/// attachments list is correct. What needs to be checked:
///
/// - That the attachments' format and samples count match the render pass layout.
/// - That the attachments have been created with the proper usage flags.
/// - That the attachments only expose one mipmap.
/// - That the attachments use identity components swizzling.
/// TODO: more stuff with aliasing
///
pub unsafe trait RenderPassDescAttachmentsList<A>: RenderPassDesc {
    /// The "compiled" list of attachments.
    type List: AttachmentsList;

    /// Decodes a `A` into a list of attachments.
    ///
    /// Checks that the attachments match the render pass, and returns a list. Returns an error if
    /// one of the attachments is wrong.
    fn check_attachments_list(&self, A) -> Result<Self::List, FramebufferCreationError>;
}

unsafe impl<A, T> RenderPassDescAttachmentsList<A> for T
    where T: SafeDeref, T::Target: RenderPassDescAttachmentsList<A>
{
    type List = <T::Target as RenderPassDescAttachmentsList<A>>::List;

    #[inline]
    fn check_attachments_list(&self, atch: A) -> Result<Self::List, FramebufferCreationError> {
        (**self).check_attachments_list(atch)
    }
}

/// Extension trait for `RenderPassDesc`. Defines which types are allowed as a list of clear values.
///
/// When the user enters a render pass, they need to pass a list of clear values to apply to
/// the attachments of the framebuffer. To do so, the `RenderPassDesc` of the framebuffer must
/// implement `RenderPassClearValues<C>` where `C` is the parameter that the user passed. The
/// trait method is then responsible for checking the correctness of these values and turning
/// them into a list that can be processed by vulkano.
///
/// Only the attachments whose `LoadOp` is `Clear` should appear in the list returned by the
/// method. Other attachments simply should not appear. TODO: check that this is correct
/// For example if attachments 1, 2 and 4 are `Clear` and attachments 0 and 3 are `Load`, then
/// the list returned by the function must have three elements which are the clear values of
/// attachments 1, 2 and 4.
///
/// # Safety
///
/// This trait is unsafe because vulkano doesn't check whether the clear value is in a format that
/// matches the attachment.
///
pub unsafe trait RenderPassClearValues<C>: RenderPassDesc {
    /// Decodes a `C` into a list of clear values where each element corresponds
    /// to an attachment. The size of the returned iterator must be the same as the number of
    /// attachments.
    ///
    /// The format of the clear value **must** match the format of the attachment. Attachments
    /// that are not loaded with `LoadOp::Clear` must have an entry equal to `ClearValue::None`.
    // TODO: meh for boxing
    fn convert_clear_values(&self, C) -> Box<Iterator<Item = ClearValue>>;
}

unsafe impl<T, C> RenderPassClearValues<C> for T
    where T: SafeDeref, T::Target: RenderPassClearValues<C>
{
    #[inline]
    fn convert_clear_values(&self, vals: C) -> Box<Iterator<Item = ClearValue>> {
        (**self).convert_clear_values(vals)
    }
}

/*unsafe impl<R: ?Sized> RenderPassClearValues<Vec<ClearValue>> for R where R: RenderPassDesc {
    #[inline]
    fn convert_clear_values(&self, vals: Vec<ClearValue>) -> Box<Iterator<Item = ClearValue>> {
        Box::new(vals.into_iter())
    }
}*/

/// Extension trait for `RenderPassDesc` that checks whether a subpass of this render pass accepts
/// the output of a fragment shader.
///
/// The trait is automatically implemented for all type that implement `RenderPassDesc` and
/// `RenderPassDesc`.
// TODO: once specialization lands, this trait can be specialized for pairs that are known to
//       always be compatible
pub unsafe trait RenderPassSubpassInterface<Other: ?Sized>: RenderPassDesc
    where Other: ShaderInterfaceDef
{
    /// Returns `true` if this subpass is compatible with the fragment output definition.
    /// Also returns `false` if the subpass is out of range.
    // TODO: return proper error
    fn is_compatible_with(&self, subpass: u32, other: &Other) -> bool;
}

unsafe impl<A, B: ?Sized> RenderPassSubpassInterface<B> for A
    where A: RenderPassDesc, B: ShaderInterfaceDef
{
    fn is_compatible_with(&self, subpass: u32, other: &B) -> bool {
        let pass_descr = match RenderPassDesc::subpasses(self).skip(subpass as usize).next() {
            Some(s) => s,
            None => return false,
        };

        for element in other.elements() {
            for location in element.location.clone() {
                let attachment_id = match pass_descr.color_attachments.get(location as usize) {
                    Some(a) => a.0,
                    None => return false,
                };

                let attachment_desc = (&self).attachments().skip(attachment_id).next().unwrap();

                // FIXME: compare formats depending on the number of components and data type
                /*if attachment_desc.format != element.format {
                    return false;
                }*/
            }
        }

        true
    }
}

/// Trait implemented on render pass objects to check whether they are compatible
/// with another render pass.
///
/// The trait is automatically implemented for all type that implement `RenderPassDesc`.
// TODO: once specialization lands, this trait can be specialized for pairs that are known to
//       always be compatible
// TODO: maybe this can be unimplemented on some pairs, to provide compile-time checks?
pub unsafe trait RenderPassCompatible<Other: ?Sized>: RenderPassDesc where Other: RenderPassDesc {
    /// Returns `true` if this layout is compatible with the other layout, as defined in the
    /// `Render Pass Compatibility` section of the Vulkan specs.
    // TODO: return proper error
    fn is_compatible_with(&self, other: &Other) -> bool;
}

unsafe impl<A, B: ?Sized> RenderPassCompatible<B> for A
    where A: RenderPassDesc, B: RenderPassDesc
{
    fn is_compatible_with(&self, other: &B) -> bool {
        // FIXME:
        /*for (atch1, atch2) in (&self).attachments().zip(other.attachments()) {
            if !atch1.is_compatible_with(&atch2) {
                return false;
            }
        }*/

        return true;

        // FIXME: finish
    }
}

/// Describes an attachment that will be used in a render pass.
#[derive(Debug, Clone)]
pub struct LayoutAttachmentDescription {
    /// Format of the image that is going to be binded.
    pub format: Format,
    /// Number of samples of the image that is going to be binded.
    pub samples: u32,

    /// What the implementation should do with that attachment at the start of the renderpass.
    pub load: LoadOp,
    /// What the implementation should do with that attachment at the end of the renderpass.
    pub store: StoreOp,

    /// Layout that the image is going to be in at the start of the renderpass.
    ///
    /// The vulkano library will automatically switch to the correct layout if necessary, but it
    /// is more optimal to set this to the correct value.
    pub initial_layout: ImageLayout,

    /// Layout that the image will be transitionned to at the end of the renderpass.
    pub final_layout: ImageLayout,
}

impl LayoutAttachmentDescription {
    /// Returns true if this attachment is compatible with another attachment, as defined in the
    /// `Render Pass Compatibility` section of the Vulkan specs.
    #[inline]
    pub fn is_compatible_with(&self, other: &LayoutAttachmentDescription) -> bool {
        self.format == other.format && self.samples == other.samples
    }
}

/// Describes one of the passes of a render pass.
///
/// # Restrictions
///
/// All these restrictions are checked when the `RenderPass` object is created.
/// TODO: that's not the case ^
///
/// - The number of color attachments must be less than the limit of the physical device.
/// - All the attachments in `color_attachments` and `depth_stencil` must have the same
///   samples count.
/// - If any attachment is used as both an input attachment and a color or
///   depth/stencil attachment, then each use must use the same layout.
/// - Elements of `preserve_attachments` must not be used in any of the other members.
/// - If `resolve_attachments` is not empty, then all the resolve attachments must be attachments
///   with 1 sample and all the color attachments must have more than 1 sample.
/// - If `resolve_attachments` is not empty, all the resolve attachments must have the same format
///   as the color attachments.
/// - If the first use of an attachment in this renderpass is as an input attachment and the
///   attachment is not also used as a color or depth/stencil attachment in the same subpass,
///   then the loading operation must not be `Clear`.
///
// TODO: add tests for all these restrictions
// TODO: allow unused attachments (for example attachment 0 and 2 are used, 1 is unused)
#[derive(Debug, Clone)]
pub struct LayoutPassDescription {
    /// Indices and layouts of attachments to use as color attachments.
    pub color_attachments: Vec<(usize, ImageLayout)>,      // TODO: Vec is slow

    /// Index and layout of the attachment to use as depth-stencil attachment.
    pub depth_stencil: Option<(usize, ImageLayout)>,

    /// Indices and layouts of attachments to use as input attachments.
    pub input_attachments: Vec<(usize, ImageLayout)>,      // TODO: Vec is slow

    /// If not empty, each color attachment will be resolved into each corresponding entry of
    /// this list.
    ///
    /// If this value is not empty, it **must** be the same length as `color_attachments`.
    pub resolve_attachments: Vec<(usize, ImageLayout)>,      // TODO: Vec is slow

    /// Indices of attachments that will be preserved during this pass.
    pub preserve_attachments: Vec<usize>,      // TODO: Vec is slow
}

/// Describes a dependency between two passes of a render pass.
///
/// The implementation is allowed to change the order of the passes within a render pass, unless
/// you specify that there exists a dependency between two passes (ie. the result of one will be
/// used as the input of another one).
#[derive(Debug, Clone)]
pub struct LayoutPassDependencyDescription {
    /// Index of the subpass that writes the data that `destination_subpass` is going to use.
    pub source_subpass: usize,

    /// Index of the subpass that reads the data that `source_subpass` wrote.
    pub destination_subpass: usize,

    /// The pipeline stages that must be finished on the previous subpass before the destination
    /// subpass can start.
    pub src_stages: PipelineStages,

    /// The pipeline stages of the destination subpass that must wait for the source to be finished.
    /// Stages that are earlier of the stages specified here can start before the source is
    /// finished.
    pub dst_stages: PipelineStages,

    /// The way the source subpass accesses the attachments on which we depend.
    pub src_access: AccessFlagBits,

    /// The way the destination subpass accesses the attachments on which we depend.
    pub dst_access: AccessFlagBits,

    /// If false, then the whole subpass must be finished for the next one to start. If true, then
    /// the implementation can start the new subpass for some given pixels as long as the previous
    /// subpass is finished for these given pixels.
    ///
    /// In other words, if the previous subpass has some side effects on other parts of an
    /// attachment, then you sould set it to false.
    ///
    /// Passing `false` is always safer than passing `true`, but in practice you rarely need to
    /// pass `false`.
    pub by_region: bool,
}

/// Describes what the implementation should do with an attachment after all the subpasses have
/// completed.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum StoreOp {
    /// The attachment will be stored. This is what you usually want.
    ///
    /// While this is the most intuitive option, it is also slower than `DontCare` because it can
    /// take time to write the data back to memory.
    Store = vk::ATTACHMENT_STORE_OP_STORE,

    /// What happens is implementation-specific.
    ///
    /// This is purely an optimization compared to `Store`. The implementation doesn't need to copy
    /// from the internal cache to the memory, which saves memory bandwidth.
    ///
    /// This doesn't mean that the data won't be copied, as an implementation is also free to not
    /// use a cache and write the output directly in memory. In other words, the content of the
    /// image will be undefined.
    DontCare = vk::ATTACHMENT_STORE_OP_DONT_CARE,
}

/// Describes what the implementation should do with an attachment at the start of the subpass.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum LoadOp {
    /// The content of the attachment will be loaded from memory. This is what you want if you want
    /// to draw over something existing.
    ///
    /// While this is the most intuitive option, it is also the slowest because it uses a lot of
    /// memory bandwidth.
    Load = vk::ATTACHMENT_LOAD_OP_LOAD,

    /// The content of the attachment will be filled by the implementation with a uniform value
    /// that you must provide when you start drawing.
    ///
    /// This is what you usually use at the start of a frame, in order to reset the content of
    /// the color, depth and/or stencil buffers.
    ///
    /// See the `draw_inline` and `draw_secondary` methods of `PrimaryComputeBufferBuilder`.
    Clear = vk::ATTACHMENT_LOAD_OP_CLEAR,

    /// The attachment will have undefined content.
    ///
    /// This is what you should use for attachments that you intend to entirely cover with draw
    /// commands.
    /// If you are going to fill the attachment with a uniform value, it is better to use `Clear`
    /// instead.
    DontCare = vk::ATTACHMENT_LOAD_OP_DONT_CARE,
}

/// Represents a subpass within a `RenderPassAbstract` object.
///
/// This struct doesn't correspond to anything in Vulkan. It is simply an equivalent to a
/// tuple of a render pass and subpass index. Contrary to a tuple, however, the existence of the
/// subpass is checked when the object is created. When you have a `Subpass` you are guaranteed
/// that the given subpass does exist.
#[derive(Debug, Copy, Clone)]
pub struct Subpass<L> {
    render_pass: L,
    subpass_id: u32,
}

impl<L> Subpass<L> where L: RenderPassDesc {
    /// Returns a handle that represents a subpass of a render pass.
    #[inline]
    pub fn from(render_pass: L, id: u32) -> Option<Subpass<L>> {
        if (id as usize) < render_pass.num_subpasses() {
            Some(Subpass {
                render_pass: render_pass,
                subpass_id: id,
            })

        } else {
            None
        }
    }

    /// Returns the number of color attachments in this subpass.
    #[inline]
    pub fn num_color_attachments(&self) -> u32 {
        self.render_pass.num_color_attachments(self.subpass_id).unwrap()
    }

    /// Returns true if the subpass has a depth attachment or a depth-stencil attachment.
    #[inline]
    pub fn has_depth(&self) -> bool {
        self.render_pass.has_depth(self.subpass_id).unwrap()
    }

    /// Returns true if the subpass has a depth attachment or a depth-stencil attachment whose
    /// layout is not `DepthStencilReadOnlyOptimal`.
    #[inline]
    pub fn has_writable_depth(&self) -> bool {
        self.render_pass.has_writable_depth(self.subpass_id).unwrap()
    }

    /// Returns true if the subpass has a stencil attachment or a depth-stencil attachment.
    #[inline]
    pub fn has_stencil(&self) -> bool {
        self.render_pass.has_stencil(self.subpass_id).unwrap()
    }

    /// Returns true if the subpass has a stencil attachment or a depth-stencil attachment whose
    /// layout is not `DepthStencilReadOnlyOptimal`.
    #[inline]
    pub fn has_writable_stencil(&self) -> bool {
        self.render_pass.has_writable_stencil(self.subpass_id).unwrap()
    }

    /// Returns true if the subpass has any color or depth/stencil attachment.
    #[inline]
    pub fn has_color_or_depth_stencil_attachment(&self) -> bool {
        self.num_color_attachments() >= 1 ||
        self.render_pass.has_depth_stencil_attachment(self.subpass_id).unwrap() != (false, false)
    }

    /// Returns the number of samples in the color and/or depth/stencil attachments. Returns `None`
    /// if there is no such attachment in this subpass.
    #[inline]
    pub fn num_samples(&self) -> Option<u32> {
        self.render_pass.num_samples(self.subpass_id)
    }
}

impl<L> Subpass<L> {
    /// Returns the render pass of this subpass.
    #[inline]
    pub fn render_pass(&self) -> &L {
        &self.render_pass
    }

    /// Returns the index of this subpass within the renderpass.
    #[inline]
    pub fn index(&self) -> u32 {
        self.subpass_id
    }
}

impl<L> Into<(L, u32)> for Subpass<L> {
    #[inline]
    fn into(self) -> (L, u32) {
        (self.render_pass, self.subpass_id)
    }
}
