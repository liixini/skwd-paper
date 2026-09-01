#include <stdint.h>
#include <string.h>

#include <libavutil/hwcontext.h>
#include <libavutil/hwcontext_vulkan.h>
#include <libavutil/frame.h>
#include <libavutil/version.h>

#if defined(FF_API_VULKAN_FIXED_QUEUES)
#define SKWD_AV_VK_HAS_FIXED_QUEUES FF_API_VULKAN_FIXED_QUEUES
#else
#define SKWD_AV_VK_HAS_FIXED_QUEUES \
    (LIBAVUTIL_VERSION_INT >= AV_VERSION_INT(57, 28, 100) && \
     LIBAVUTIL_VERSION_MAJOR < 61)
#endif

#if defined(FF_API_VULKAN_SYNC_QUEUES)
#define SKWD_AV_VK_HAS_SYNC_QUEUES FF_API_VULKAN_SYNC_QUEUES
#else
#define SKWD_AV_VK_HAS_SYNC_QUEUES \
    (LIBAVUTIL_VERSION_INT >= AV_VERSION_INT(58, 2, 100) && \
     LIBAVUTIL_VERSION_MAJOR < 62)
#endif

#define SKWD_AV_VK_HAS_QUEUE_FAMILIES \
    (LIBAVUTIL_VERSION_INT >= AV_VERSION_INT(59, 34, 100))

#define SKWD_AV_VK_HAS_FRAME_LOCK \
    (LIBAVUTIL_VERSION_INT >= AV_VERSION_INT(58, 11, 100))

#if SKWD_AV_VK_HAS_SYNC_QUEUES
typedef void (*SkwdQueueLock)(struct AVHWDeviceContext *, uint32_t, uint32_t);
#endif

int skwd_av_vk_device_configure(
    AVVulkanDeviceContext *context,
    uintptr_t get_proc_addr,
    uint64_t instance,
    uint64_t physical_device,
    uint64_t device,
    const void *features,
    const char *const *instance_extensions,
    int instance_extension_count,
    const char *const *device_extensions,
    int device_extension_count,
    uintptr_t lock_queue,
    uintptr_t unlock_queue
) {
    if (!context || !features) {
        return 0;
    }
    context->get_proc_addr = (PFN_vkGetInstanceProcAddr)get_proc_addr;
    context->inst = (VkInstance)(uintptr_t)instance;
    context->phys_dev = (VkPhysicalDevice)(uintptr_t)physical_device;
    context->act_dev = (VkDevice)(uintptr_t)device;
    memcpy(&context->device_features, features, sizeof(context->device_features));
    context->enabled_inst_extensions = instance_extensions;
    context->nb_enabled_inst_extensions = instance_extension_count;
    context->enabled_dev_extensions = device_extensions;
    context->nb_enabled_dev_extensions = device_extension_count;
#if SKWD_AV_VK_HAS_SYNC_QUEUES
    int queue_sync = lock_queue != 0 && unlock_queue != 0;
    context->lock_queue = queue_sync ? (SkwdQueueLock)lock_queue : 0;
    context->unlock_queue = queue_sync ? (SkwdQueueLock)unlock_queue : 0;
#else
    (void)lock_queue;
    (void)unlock_queue;
#endif
#if SKWD_AV_VK_HAS_QUEUE_FAMILIES
    context->nb_qf = 0;
#endif
#if SKWD_AV_VK_HAS_SYNC_QUEUES
    return queue_sync ? 2 : 1;
#else
    return 1;
#endif
}

int skwd_av_vk_device_add_queue(
    AVVulkanDeviceContext *context,
    int index,
    uint32_t flags,
    uint32_t video_caps
) {
    if (!context) {
        return 0;
    }
#if SKWD_AV_VK_HAS_QUEUE_FAMILIES
    if (context->nb_qf < 0 ||
        (size_t)context->nb_qf >= sizeof(context->qf) / sizeof(context->qf[0])) {
        return 0;
    }
    context->qf[context->nb_qf++] = (AVVulkanDeviceQueueFamily) {
        .idx = index,
        .num = 1,
        .flags = (VkQueueFlagBits)flags,
        .video_caps = (VkVideoCodecOperationFlagBitsKHR)video_caps,
    };
#else
    (void)index;
    (void)flags;
    (void)video_caps;
#endif
    return 1;
}

void skwd_av_vk_device_set_legacy_queues(
    AVVulkanDeviceContext *context,
    int graphics,
    int transfer,
    int compute,
    int encode,
    int decode
) {
#if SKWD_AV_VK_HAS_FIXED_QUEUES
    context->queue_family_index = graphics;
    context->nb_graphics_queues = graphics >= 0;
    context->queue_family_tx_index = transfer;
    context->nb_tx_queues = transfer >= 0;
    context->queue_family_comp_index = compute;
    context->nb_comp_queues = compute >= 0;
    context->queue_family_encode_index = encode;
    context->nb_encode_queues = encode >= 0;
    context->queue_family_decode_index = decode;
    context->nb_decode_queues = decode >= 0;
#else
    (void)context;
    (void)graphics;
    (void)transfer;
    (void)compute;
    (void)encode;
    (void)decode;
#endif
}

unsigned skwd_av_vk_frame_image_count(const void *pointer) {
    const AVVkFrame *frame = pointer;
    unsigned count = 0;
    if (!frame) {
        return 0;
    }
    while (count < AV_NUM_DATA_POINTERS && frame->img[count] != VK_NULL_HANDLE) {
        ++count;
    }
    return count;
}

uint64_t skwd_av_vk_frame_image(const void *pointer, unsigned index) {
    const AVVkFrame *frame = pointer;
    if (!frame || index >= AV_NUM_DATA_POINTERS) {
        return 0;
    }
    return (uint64_t)frame->img[index];
}

int32_t skwd_av_vk_frame_layout(const void *pointer, unsigned index) {
    const AVVkFrame *frame = pointer;
    if (!frame || index >= AV_NUM_DATA_POINTERS) {
        return 0;
    }
    return (int32_t)frame->layout[index];
}

uint64_t skwd_av_vk_frame_semaphore(const void *pointer, unsigned index) {
    const AVVkFrame *frame = pointer;
    if (!frame || index >= AV_NUM_DATA_POINTERS) {
        return 0;
    }
    return (uint64_t)frame->sem[index];
}

uint64_t skwd_av_vk_frame_semaphore_value(const void *pointer, unsigned index) {
    const AVVkFrame *frame = pointer;
    if (!frame || index >= AV_NUM_DATA_POINTERS) {
        return 0;
    }
    return frame->sem_value[index];
}

int32_t skwd_av_vk_frame_format(const AVFrame *video, unsigned index) {
    if (!video || !video->hw_frames_ctx || !video->hw_frames_ctx->data ||
        index >= AV_NUM_DATA_POINTERS) {
        return 0;
    }
    AVHWFramesContext *frames = (AVHWFramesContext *)video->hw_frames_ctx->data;
    AVVulkanFramesContext *context = frames->hwctx;
    if (!context) {
        return 0;
    }
    return (int32_t)context->format[index];
}

int skwd_av_vk_frame_lock(const AVFrame *video) {
#if SKWD_AV_VK_HAS_FRAME_LOCK
    if (!video || !video->hw_frames_ctx || !video->hw_frames_ctx->data || !video->data[0]) {
        return 0;
    }
    AVHWFramesContext *frames = (AVHWFramesContext *)video->hw_frames_ctx->data;
    AVVulkanFramesContext *context = frames->hwctx;
    if (!context || !context->lock_frame || !context->unlock_frame) {
        return 0;
    }
    context->lock_frame(frames, (AVVkFrame *)video->data[0]);
    return 1;
#else
    (void)video;
    return 0;
#endif
}

void skwd_av_vk_frame_unlock(const AVFrame *video) {
#if SKWD_AV_VK_HAS_FRAME_LOCK
    if (!video || !video->hw_frames_ctx || !video->hw_frames_ctx->data || !video->data[0]) {
        return;
    }
    AVHWFramesContext *frames = (AVHWFramesContext *)video->hw_frames_ctx->data;
    AVVulkanFramesContext *context = frames->hwctx;
    if (context && context->unlock_frame) {
        context->unlock_frame(frames, (AVVkFrame *)video->data[0]);
    }
#else
    (void)video;
#endif
}

void skwd_av_vk_frame_set_state(void *pointer, unsigned index, int32_t layout, uint64_t access) {
    AVVkFrame *frame = pointer;
    if (!frame || index >= AV_NUM_DATA_POINTERS) {
        return;
    }
    frame->layout[index] = (VkImageLayout)layout;
#if LIBAVUTIL_VERSION_MAJOR >= 61
    frame->access[index] = (VkAccessFlags2)access;
#else
    frame->access[index] = (VkAccessFlagBits)access;
#endif
}

void skwd_av_vk_frame_set_state_and_semaphore(
    void *pointer,
    unsigned index,
    int32_t layout,
    uint64_t access,
    uint64_t semaphore_value
) {
    skwd_av_vk_frame_set_state(pointer, index, layout, access);
#if SKWD_AV_VK_HAS_FRAME_LOCK
    AVVkFrame *frame = pointer;
    if (frame && index < AV_NUM_DATA_POINTERS) {
        frame->sem_value[index] = semaphore_value;
    }
#else
    (void)semaphore_value;
#endif
}
