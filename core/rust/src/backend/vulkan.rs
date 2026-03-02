use std::ffi::CString;

use ash::vk;

use crate::api::Status;

use super::filter::{FilterPass, FilterPipeline, FilterReport};

const WORKGROUP_SIZE_X: u32 = 16;
const WORKGROUP_SIZE_Y: u32 = 16;
const GAUSSIAN_SHADER_WGSL: &str = r#"
@group(0) @binding(0)
var<storage, read> src_pixels: array<u32>;

@group(0) @binding(1)
var<storage, read_write> dst_pixels: array<u32>;

@group(0) @binding(2)
var<storage, read> params_words: array<u32>;

fn unpack_rgba(v: u32) -> vec4<f32> {
    let r = f32(v & 0xffu);
    let g = f32((v >> 8u) & 0xffu);
    let b = f32((v >> 16u) & 0xffu);
    let a = f32((v >> 24u) & 0xffu);
    return vec4<f32>(r, g, b, a);
}

fn pack_rgba(v: vec4<f32>) -> u32 {
    let c = clamp(v, vec4<f32>(0.0), vec4<f32>(255.0));
    let r = u32(round(c.x));
    let g = u32(round(c.y));
    let b = u32(round(c.z));
    let a = u32(round(c.w));
    return (r & 0xffu)
        | ((g & 0xffu) << 8u)
        | ((b & 0xffu) << 16u)
        | ((a & 0xffu) << 24u);
}

fn gaussian_weight(dist2: f32, sigma: f32) -> f32 {
    return exp(-dist2 / (2.0 * sigma * sigma));
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = params_words[0u];
    let height = params_words[1u];
    if (gid.x >= width || gid.y >= height) {
        return;
    }

    let radius = i32(params_words[2u]);
    let sigma = max(bitcast<f32>(params_words[3u]), 0.01);
    let x = i32(gid.x);
    let y = i32(gid.y);

    var accum = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    var wsum = 0.0;

    for (var dy = -radius; dy <= radius; dy = dy + 1) {
        for (var dx = -radius; dx <= radius; dx = dx + 1) {
            let sx = clamp(x + dx, 0, i32(width) - 1);
            let sy = clamp(y + dy, 0, i32(height) - 1);
            let idx = u32(sy) * width + u32(sx);
            let w = gaussian_weight(f32(dx * dx + dy * dy), sigma);
            accum = accum + unpack_rgba(src_pixels[idx]) * w;
            wsum = wsum + w;
        }
    }

    let out = accum / max(wsum, 0.0001);
    let out_idx = gid.y * width + gid.x;
    dst_pixels[out_idx] = pack_rgba(out);
}
"#;

#[derive(Debug, Clone)]
pub struct VulkanError {
    message: String,
}

impl VulkanError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VulkanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for VulkanError {}

impl From<VulkanError> for Status {
    fn from(_value: VulkanError) -> Self {
        Status::InternalError
    }
}

struct TransientBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
}

struct VulkanContext {
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    command_pool: vk::CommandPool,
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_pipeline(self.pipeline, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

pub struct VulkanBackend {
    context: VulkanContext,
    filter_pipeline: FilterPipeline,
}

impl std::fmt::Debug for VulkanBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanBackend")
            .field("backend", &"vulkan-compute")
            .field("gpu_path_active", &self.gpu_path_active())
            .field("pass_count", &self.filter_pipeline.passes.len())
            .finish()
    }
}

impl VulkanBackend {
    pub fn new() -> Result<Self, VulkanError> {
        let context = create_vulkan_context()?;
        Ok(Self {
            context,
            filter_pipeline: FilterPipeline::default(),
        })
    }

    pub fn backend_name(&self) -> &'static str {
        "vulkan-compute"
    }

    pub fn gpu_path_active(&self) -> bool {
        true
    }

    pub fn set_filter_pipeline(&mut self, pipeline: FilterPipeline) {
        self.filter_pipeline = pipeline;
    }

    pub fn filter_pipeline(&self) -> &FilterPipeline {
        &self.filter_pipeline
    }

    pub fn process_frame_rgba(
        &self,
        width: u32,
        height: u32,
        pixels_rgba8: &mut [u8],
    ) -> Result<FilterReport, Status> {
        validate_rgba_len(width, height, pixels_rgba8.len())?;
        if self.filter_pipeline.passes.is_empty() {
            return Ok(FilterReport::default());
        }

        let mut words = pack_rgba_words(pixels_rgba8);
        let mut report = FilterReport::default();

        for pass in &self.filter_pipeline.passes {
            match pass {
                FilterPass::GaussianBlur(cfg) => {
                    let cfg = cfg.normalized();
                    if cfg.radius == 0 {
                        continue;
                    }
                    let mut out_words = vec![0u32; words.len()];
                    self.dispatch_gaussian_pass(
                        width,
                        height,
                        &words,
                        &mut out_words,
                        cfg.radius,
                        cfg.sigma,
                    )
                    .map_err(Status::from)?;
                    words = out_words;
                    report.executed_passes = report.executed_passes.saturating_add(1);
                    report.frost_passes = report.frost_passes.saturating_add(1);
                }
            }
        }

        unpack_rgba_words(&words, pixels_rgba8);
        Ok(report)
    }

    fn dispatch_gaussian_pass(
        &self,
        width: u32,
        height: u32,
        src_words: &[u32],
        dst_words: &mut [u32],
        radius: u32,
        sigma: f32,
    ) -> Result<(), VulkanError> {
        if src_words.len() != dst_words.len() {
            return Err(VulkanError::new("vulkan_blur_buffer_len_mismatch"));
        }
        let pixel_count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| VulkanError::new("vulkan_blur_pixel_count_overflow"))?;
        if pixel_count != src_words.len() {
            return Err(VulkanError::new("vulkan_blur_pixel_count_mismatch"));
        }

        let src_bytes = words_to_le_bytes(src_words);
        let mut dst_bytes = vec![0u8; src_bytes.len()];
        let params = [width, height, radius, sigma.to_bits()];
        let params_bytes = words_to_le_bytes(&params);

        self.run_compute_dispatch(width, height, &src_bytes, &params_bytes, &mut dst_bytes)?;

        let out_words = le_bytes_to_words(&dst_bytes)?;
        if out_words.len() != dst_words.len() {
            return Err(VulkanError::new("vulkan_blur_output_len_mismatch"));
        }
        dst_words.copy_from_slice(&out_words);
        Ok(())
    }

    fn run_compute_dispatch(
        &self,
        width: u32,
        height: u32,
        src_bytes: &[u8],
        params_bytes: &[u8],
        dst_bytes: &mut [u8],
    ) -> Result<(), VulkanError> {
        let mut input = None;
        let mut output = None;
        let mut params = None;
        let mut descriptor_set = vk::DescriptorSet::null();
        let mut command_buffer = vk::CommandBuffer::null();
        let mut fence = vk::Fence::null();

        let result = (|| -> Result<(), VulkanError> {
            let input_buffer = create_host_visible_storage_buffer(
                &self.context.device,
                &self.context.memory_properties,
                src_bytes.len() as vk::DeviceSize,
            )?;
            write_memory_bytes(&self.context.device, input_buffer.memory, src_bytes)?;
            input = Some(input_buffer);

            let output_buffer = create_host_visible_storage_buffer(
                &self.context.device,
                &self.context.memory_properties,
                dst_bytes.len() as vk::DeviceSize,
            )?;
            output = Some(output_buffer);

            let params_buffer = create_host_visible_storage_buffer(
                &self.context.device,
                &self.context.memory_properties,
                params_bytes.len() as vk::DeviceSize,
            )?;
            write_memory_bytes(&self.context.device, params_buffer.memory, params_bytes)?;
            params = Some(params_buffer);

            let set_layouts = [self.context.descriptor_set_layout];
            let alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.context.descriptor_pool)
                .set_layouts(&set_layouts);
            descriptor_set = unsafe {
                self.context
                    .device
                    .allocate_descriptor_sets(&alloc_info)
                    .map_err(|e| {
                        VulkanError::new(format!("vulkan_allocate_descriptor_set_failed:{e:?}"))
                    })?
            }
            .into_iter()
            .next()
            .ok_or_else(|| VulkanError::new("vulkan_descriptor_set_empty"))?;

            let input_info = [vk::DescriptorBufferInfo::default()
                .buffer(input.as_ref().expect("input exists").buffer)
                .offset(0)
                .range(input.as_ref().expect("input exists").size)];
            let output_info = [vk::DescriptorBufferInfo::default()
                .buffer(output.as_ref().expect("output exists").buffer)
                .offset(0)
                .range(output.as_ref().expect("output exists").size)];
            let params_info = [vk::DescriptorBufferInfo::default()
                .buffer(params.as_ref().expect("params exists").buffer)
                .offset(0)
                .range(params.as_ref().expect("params exists").size)];

            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(descriptor_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&input_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(descriptor_set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&output_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(descriptor_set)
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&params_info),
            ];
            unsafe {
                self.context.device.update_descriptor_sets(&writes, &[]);
            }

            let command_alloc = vk::CommandBufferAllocateInfo::default()
                .command_pool(self.context.command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            command_buffer = unsafe {
                self.context
                    .device
                    .allocate_command_buffers(&command_alloc)
                    .map_err(|e| {
                        VulkanError::new(format!("vulkan_allocate_command_buffer_failed:{e:?}"))
                    })?
            }
            .into_iter()
            .next()
            .ok_or_else(|| VulkanError::new("vulkan_command_buffer_empty"))?;

            let begin_info = vk::CommandBufferBeginInfo::default();
            unsafe {
                self.context
                    .device
                    .begin_command_buffer(command_buffer, &begin_info)
                    .map_err(|e| {
                        VulkanError::new(format!("vulkan_begin_command_buffer_failed:{e:?}"))
                    })?;

                self.context.device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::COMPUTE,
                    self.context.pipeline,
                );
                self.context.device.cmd_bind_descriptor_sets(
                    command_buffer,
                    vk::PipelineBindPoint::COMPUTE,
                    self.context.pipeline_layout,
                    0,
                    &[descriptor_set],
                    &[],
                );

                let groups_x = width.div_ceil(WORKGROUP_SIZE_X);
                let groups_y = height.div_ceil(WORKGROUP_SIZE_Y);
                self.context.device.cmd_dispatch(
                    command_buffer,
                    groups_x.max(1),
                    groups_y.max(1),
                    1,
                );

                self.context
                    .device
                    .end_command_buffer(command_buffer)
                    .map_err(|e| {
                        VulkanError::new(format!("vulkan_end_command_buffer_failed:{e:?}"))
                    })?;
            }

            let fence_info = vk::FenceCreateInfo::default();
            fence = unsafe {
                self.context
                    .device
                    .create_fence(&fence_info, None)
                    .map_err(|e| VulkanError::new(format!("vulkan_create_fence_failed:{e:?}")))?
            };

            let cmd_bufs = [command_buffer];
            let submit_infos = [vk::SubmitInfo::default().command_buffers(&cmd_bufs)];
            unsafe {
                self.context
                    .device
                    .queue_submit(self.context.queue, &submit_infos, fence)
                    .map_err(|e| VulkanError::new(format!("vulkan_queue_submit_failed:{e:?}")))?;
                self.context
                    .device
                    .wait_for_fences(&[fence], true, u64::MAX)
                    .map_err(|e| VulkanError::new(format!("vulkan_wait_fence_failed:{e:?}")))?;
            }

            read_memory_bytes(
                &self.context.device,
                output.as_ref().expect("output exists").memory,
                dst_bytes,
            )?;

            Ok(())
        })();

        if fence != vk::Fence::null() {
            unsafe {
                self.context.device.destroy_fence(fence, None);
            }
        }
        if command_buffer != vk::CommandBuffer::null() {
            unsafe {
                self.context
                    .device
                    .free_command_buffers(self.context.command_pool, &[command_buffer]);
            }
        }
        if descriptor_set != vk::DescriptorSet::null() {
            unsafe {
                let _ = self
                    .context
                    .device
                    .free_descriptor_sets(self.context.descriptor_pool, &[descriptor_set]);
            }
        }

        destroy_transient_buffer(&self.context.device, &mut params);
        destroy_transient_buffer(&self.context.device, &mut output);
        destroy_transient_buffer(&self.context.device, &mut input);

        result
    }
}

fn create_vulkan_context() -> Result<VulkanContext, VulkanError> {
    let entry = unsafe {
        ash::Entry::load().map_err(|e| VulkanError::new(format!("vulkan_load_entry_failed:{e}")))?
    };

    let app_name =
        CString::new("DirectScreenAPI").map_err(|_| VulkanError::new("vulkan_app_name_invalid"))?;
    let engine_name = CString::new("DirectScreenCore")
        .map_err(|_| VulkanError::new("vulkan_engine_name_invalid"))?;
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .application_version(1)
        .engine_name(&engine_name)
        .engine_version(1)
        .api_version(vk::make_api_version(0, 1, 1, 0));
    let instance_info = vk::InstanceCreateInfo::default().application_info(&app_info);

    let instance = unsafe {
        entry
            .create_instance(&instance_info, None)
            .map_err(|e| VulkanError::new(format!("vulkan_create_instance_failed:{e:?}")))?
    };

    let mut device_opt: Option<ash::Device> = None;
    let mut descriptor_pool = vk::DescriptorPool::null();
    let mut descriptor_set_layout = vk::DescriptorSetLayout::null();
    let mut pipeline_layout = vk::PipelineLayout::null();
    let mut pipeline = vk::Pipeline::null();
    let mut command_pool = vk::CommandPool::null();
    let mut queue = vk::Queue::null();
    let mut queue_family_index = 0u32;
    let mut memory_properties = vk::PhysicalDeviceMemoryProperties::default();

    let result = (|| -> Result<(), VulkanError> {
        let physical_devices = unsafe {
            instance.enumerate_physical_devices().map_err(|e| {
                VulkanError::new(format!("vulkan_enumerate_physical_devices_failed:{e:?}"))
            })?
        };
        let Some((physical_device, family_index)) = physical_devices.iter().find_map(|pd| {
            let props = unsafe { instance.get_physical_device_queue_family_properties(*pd) };
            props
                .iter()
                .enumerate()
                .find(|(_, p)| p.queue_flags.contains(vk::QueueFlags::COMPUTE))
                .map(|(idx, _)| (*pd, idx as u32))
        }) else {
            return Err(VulkanError::new("vulkan_no_compute_queue_found"));
        };

        queue_family_index = family_index;
        memory_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        let queue_priorities = [1.0f32];
        let queue_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities)];
        let device_info = vk::DeviceCreateInfo::default().queue_create_infos(&queue_infos);
        let device = unsafe {
            instance
                .create_device(physical_device, &device_info, None)
                .map_err(|e| VulkanError::new(format!("vulkan_create_device_failed:{e:?}")))?
        };
        queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        device_opt = Some(device);

        let device_ref = device_opt.as_ref().expect("device initialized");

        let layout_bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
        ];
        let descriptor_layout_info =
            vk::DescriptorSetLayoutCreateInfo::default().bindings(&layout_bindings);
        descriptor_set_layout = unsafe {
            device_ref
                .create_descriptor_set_layout(&descriptor_layout_info, None)
                .map_err(|e| {
                    VulkanError::new(format!("vulkan_create_descriptor_layout_failed:{e:?}"))
                })?
        };

        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(192)];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::default()
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(64)
            .pool_sizes(&pool_sizes);
        descriptor_pool = unsafe {
            device_ref
                .create_descriptor_pool(&descriptor_pool_info, None)
                .map_err(|e| {
                    VulkanError::new(format!("vulkan_create_descriptor_pool_failed:{e:?}"))
                })?
        };

        let set_layouts = [descriptor_set_layout];
        let pipeline_layout_info =
            vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts);
        pipeline_layout = unsafe {
            device_ref
                .create_pipeline_layout(&pipeline_layout_info, None)
                .map_err(|e| {
                    VulkanError::new(format!("vulkan_create_pipeline_layout_failed:{e:?}"))
                })?
        };

        let shader_words = compile_shader_words()?;
        let shader_module_info = vk::ShaderModuleCreateInfo::default().code(&shader_words);
        let shader_module = unsafe {
            device_ref
                .create_shader_module(&shader_module_info, None)
                .map_err(|e| {
                    VulkanError::new(format!("vulkan_create_shader_module_failed:{e:?}"))
                })?
        };

        let entry_point =
            CString::new("main").map_err(|_| VulkanError::new("vulkan_entry_point_invalid"))?;
        let stage_info = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&entry_point);
        let compute_infos = [vk::ComputePipelineCreateInfo::default()
            .stage(stage_info)
            .layout(pipeline_layout)];

        let created_pipeline = unsafe {
            match device_ref.create_compute_pipelines(
                vk::PipelineCache::null(),
                &compute_infos,
                None,
            ) {
                Ok(mut pipelines) => pipelines
                    .drain(..)
                    .next()
                    .ok_or_else(|| VulkanError::new("vulkan_compute_pipeline_empty"))?,
                Err((mut pipelines, err)) => {
                    for p in pipelines.drain(..) {
                        device_ref.destroy_pipeline(p, None);
                    }
                    return Err(VulkanError::new(format!(
                        "vulkan_create_compute_pipeline_failed:{err:?}"
                    )));
                }
            }
        };
        unsafe {
            device_ref.destroy_shader_module(shader_module, None);
        }
        pipeline = created_pipeline;

        let command_pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        command_pool = unsafe {
            device_ref
                .create_command_pool(&command_pool_info, None)
                .map_err(|e| VulkanError::new(format!("vulkan_create_command_pool_failed:{e:?}")))?
        };

        Ok(())
    })();

    if let Err(err) = result {
        if let Some(device) = device_opt.as_ref() {
            unsafe {
                if command_pool != vk::CommandPool::null() {
                    device.destroy_command_pool(command_pool, None);
                }
                if pipeline != vk::Pipeline::null() {
                    device.destroy_pipeline(pipeline, None);
                }
                if pipeline_layout != vk::PipelineLayout::null() {
                    device.destroy_pipeline_layout(pipeline_layout, None);
                }
                if descriptor_set_layout != vk::DescriptorSetLayout::null() {
                    device.destroy_descriptor_set_layout(descriptor_set_layout, None);
                }
                if descriptor_pool != vk::DescriptorPool::null() {
                    device.destroy_descriptor_pool(descriptor_pool, None);
                }
                device.destroy_device(None);
            }
        }
        unsafe {
            instance.destroy_instance(None);
        }
        return Err(err);
    }

    let Some(device) = device_opt else {
        unsafe {
            instance.destroy_instance(None);
        }
        return Err(VulkanError::new("vulkan_device_not_initialized"));
    };

    Ok(VulkanContext {
        instance,
        device,
        queue,
        memory_properties,
        descriptor_pool,
        descriptor_set_layout,
        pipeline_layout,
        pipeline,
        command_pool,
    })
}

fn compile_shader_words() -> Result<Vec<u32>, VulkanError> {
    let module = naga::front::wgsl::parse_str(GAUSSIAN_SHADER_WGSL)
        .map_err(|e| VulkanError::new(format!("vulkan_shader_parse_failed:{e}")))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator
        .validate(&module)
        .map_err(|e| VulkanError::new(format!("vulkan_shader_validate_failed:{e}")))?;

    let options = naga::back::spv::Options {
        lang_version: (1, 0),
        flags: naga::back::spv::WriterFlags::empty(),
        ..Default::default()
    };
    let pipeline_options = naga::back::spv::PipelineOptions {
        shader_stage: naga::ShaderStage::Compute,
        entry_point: "main".to_string(),
    };
    naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline_options))
        .map_err(|e| VulkanError::new(format!("vulkan_shader_spv_emit_failed:{e}")))
}

fn create_host_visible_storage_buffer(
    device: &ash::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    size: vk::DeviceSize,
) -> Result<TransientBuffer, VulkanError> {
    let buffer_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe {
        device
            .create_buffer(&buffer_info, None)
            .map_err(|e| VulkanError::new(format!("vulkan_create_buffer_failed:{e:?}")))?
    };

    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let required_flags =
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let memory_type_index = find_memory_type_index(
        memory_properties,
        requirements.memory_type_bits,
        required_flags,
    )
    .ok_or_else(|| VulkanError::new("vulkan_memory_type_not_found"))?;

    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type_index);
    let memory = unsafe {
        device
            .allocate_memory(&alloc_info, None)
            .map_err(|e| VulkanError::new(format!("vulkan_allocate_memory_failed:{e:?}")))?
    };

    if let Err(e) = unsafe { device.bind_buffer_memory(buffer, memory, 0) } {
        unsafe {
            device.free_memory(memory, None);
            device.destroy_buffer(buffer, None);
        }
        return Err(VulkanError::new(format!(
            "vulkan_bind_buffer_memory_failed:{e:?}"
        )));
    }

    Ok(TransientBuffer {
        buffer,
        memory,
        size,
    })
}

fn destroy_transient_buffer(device: &ash::Device, buffer: &mut Option<TransientBuffer>) {
    if let Some(buf) = buffer.take() {
        unsafe {
            device.destroy_buffer(buf.buffer, None);
            device.free_memory(buf.memory, None);
        }
    }
}

fn write_memory_bytes(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    src: &[u8],
) -> Result<(), VulkanError> {
    let ptr = unsafe {
        device
            .map_memory(
                memory,
                0,
                src.len() as vk::DeviceSize,
                vk::MemoryMapFlags::empty(),
            )
            .map_err(|e| VulkanError::new(format!("vulkan_map_memory_write_failed:{e:?}")))?
    } as *mut u8;
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), ptr, src.len());
        device.unmap_memory(memory);
    }
    Ok(())
}

fn read_memory_bytes(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    dst: &mut [u8],
) -> Result<(), VulkanError> {
    let ptr = unsafe {
        device
            .map_memory(
                memory,
                0,
                dst.len() as vk::DeviceSize,
                vk::MemoryMapFlags::empty(),
            )
            .map_err(|e| VulkanError::new(format!("vulkan_map_memory_read_failed:{e:?}")))?
    } as *const u8;
    unsafe {
        let src_slice = std::slice::from_raw_parts(ptr, dst.len());
        dst.copy_from_slice(src_slice);
        device.unmap_memory(memory);
    }
    Ok(())
}

fn find_memory_type_index(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    required_flags: vk::MemoryPropertyFlags,
) -> Option<u32> {
    for idx in 0..memory_properties.memory_type_count {
        let type_mask = 1u32 << idx;
        if (type_bits & type_mask) == 0 {
            continue;
        }
        let flags = memory_properties.memory_types[idx as usize].property_flags;
        if flags.contains(required_flags) {
            return Some(idx);
        }
    }
    None
}

fn validate_rgba_len(width: u32, height: u32, len: usize) -> Result<(), Status> {
    if width == 0 || height == 0 {
        return Err(Status::InvalidArgument);
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
        .ok_or(Status::OutOfRange)?;
    if expected != len {
        return Err(Status::InvalidArgument);
    }
    Ok(())
}

fn pack_rgba_words(bytes: &[u8]) -> Vec<u32> {
    let mut out = vec![0u32; bytes.len() / 4];
    for (i, px) in bytes.chunks_exact(4).enumerate() {
        out[i] = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
    }
    out
}

fn unpack_rgba_words(words: &[u32], out_bytes: &mut [u8]) {
    for (i, px) in out_bytes.chunks_exact_mut(4).enumerate() {
        let packed = words[i].to_le_bytes();
        px.copy_from_slice(&packed);
    }
}

fn words_to_le_bytes(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(words.len() * 4);
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

fn le_bytes_to_words(bytes: &[u8]) -> Result<Vec<u32>, VulkanError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(VulkanError::new("vulkan_output_not_aligned"));
    }
    let mut out = vec![0u32; bytes.len() / 4];
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        out[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::filter::{FilterPass, GaussianBlurPass, FILTER_PASS_KIND_GAUSSIAN};

    #[test]
    fn vulkan_backend_bootstrap_processes_filter_pipeline() {
        let Ok(mut backend) = VulkanBackend::new() else {
            // CI 或本地环境可能没有可用 Vulkan 设备，跳过功能性断言。
            return;
        };

        backend.set_filter_pipeline(FilterPipeline {
            passes: vec![FilterPass::GaussianBlur(GaussianBlurPass {
                radius: 1,
                sigma: 1.0,
            })],
        });

        let mut pixels = vec![0u8; 3 * 3 * 4];
        let center = 4usize * 4usize;
        pixels[center] = 255;
        pixels[center + 1] = 255;
        pixels[center + 2] = 255;
        pixels[center + 3] = 255;

        let report = backend
            .process_frame_rgba(3, 3, &mut pixels)
            .expect("process frame");
        assert_eq!(report.frost_passes, 1);
        assert!(pixels[center] < 255);
    }

    #[test]
    fn gaussian_pass_kind_constant_stable() {
        assert_eq!(FILTER_PASS_KIND_GAUSSIAN, 1);
    }
}
