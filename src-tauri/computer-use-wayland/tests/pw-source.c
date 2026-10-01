/* Owned acceptance fixture only, not a desktop or portal implementation. */
#include <pipewire/pipewire.h>
#include <spa/param/video/format-utils.h>
#include <signal.h>
#include <unistd.h>
#include <stdio.h>
#include <stdint.h>

struct fixture {
    struct pw_main_loop *loop;
    struct pw_stream *stream;
    struct spa_source *timer;
    struct spa_video_info_raw info;
    uint64_t seq;
    uint32_t transform;
    char mode; /* s: static (no buffers), g: GAP, c: corrupt, e: empty, r: normal */
};
static void process(void *userdata) {
    struct fixture *f = userdata;
    struct pw_buffer *b = pw_stream_dequeue_buffer(f->stream);
    if (!b) return;
    struct spa_buffer *buf = b->buffer;
    const uint32_t w = f->info.size.width, h = f->info.size.height;
    const uint32_t stride = w * 4 + 16;
    if (!buf || buf->n_datas != 1 || !buf->datas[0].data || buf->datas[0].maxsize < stride * h + 8) goto done;
    uint8_t *p = (uint8_t *)buf->datas[0].data + 8;
    for (uint32_t y = 0; y < h; y++) for (uint32_t x = 0; x < w; x++) {
        uint8_t *v = p + y * stride + x * 4;
        v[0] = x; v[1] = y; v[2] = f->seq % 251; v[3] = 165;
    }
    struct spa_meta_header *header = spa_buffer_find_meta_data(buf, SPA_META_Header, sizeof(*header));
    if (header) { header->seq = f->seq; header->flags = f->mode == 'g' ? SPA_META_HEADER_FLAG_GAP : 0; header->pts = -1; }
    struct spa_meta_region *crop = spa_buffer_find_meta_data(buf, SPA_META_VideoCrop, sizeof(*crop));
    if (crop) { crop->region = SPA_REGION(2, 2, w - 4, h - 4); }
    struct spa_meta_videotransform *transform = spa_buffer_find_meta_data(buf, SPA_META_VideoTransform, sizeof(*transform));
    if (transform) transform->transform = f->transform;
    buf->datas[0].chunk->offset = 8;
    buf->datas[0].chunk->size = f->mode == 'e' ? 0 : stride * h;
    buf->datas[0].chunk->stride = stride;
    buf->datas[0].chunk->flags = f->mode == 'c' ? SPA_CHUNK_FLAG_CORRUPTED : 0;
    f->seq++;
done:
    pw_stream_queue_buffer(f->stream, b);
}
static void tick(void *userdata, uint64_t count) {
    (void)count;
    struct fixture *f = userdata;
    if (f->mode != 's') pw_stream_trigger_process(f->stream);
}
static void command(void *userdata, int fd, uint32_t mask) {
    struct fixture *f = userdata;
    char key;
    if (!(mask & SPA_IO_IN) || read(fd, &key, 1) != 1) return;
    if (key != 's' && key != 'g' && key != 'c' && key != 'e' && key != 'r') return;
    f->mode = key;
    printf("MODE=%c SEQUENCE=%lu\n", key, (unsigned long)f->seq); fflush(stdout);
}
static void state(void *userdata, enum pw_stream_state old, enum pw_stream_state next, const char *error) {
    (void)old;
    struct fixture *f = userdata;
    printf("STATE=%s\n", pw_stream_state_as_string(next)); fflush(stdout);
    if (next == PW_STREAM_STATE_PAUSED) { printf("SOURCE=%u\n", pw_stream_get_node_id(f->stream)); fflush(stdout); }
    if (next == PW_STREAM_STATE_ERROR) { fprintf(stderr, "source error: %s\n", error); pw_main_loop_quit(f->loop); }
    struct timespec first = {0, 1}, interval = {0, 40000000};
    pw_loop_update_timer(pw_main_loop_get_loop(f->loop), f->timer,
        next == PW_STREAM_STATE_STREAMING ? &first : NULL,
        next == PW_STREAM_STATE_STREAMING ? &interval : NULL, false);
}
static void format(void *userdata, uint32_t id, const struct spa_pod *pod) {
    struct fixture *f = userdata;
    if (!pod || id != SPA_PARAM_Format) return;
    if (spa_format_video_raw_parse(pod, &f->info) < 0) return;
    uint8_t bytes[1024]; struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(bytes, sizeof(bytes));
    uint32_t stride = f->info.size.width * 4 + 16;
    const struct spa_pod *params[] = {
        spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
            SPA_PARAM_BUFFERS_buffers, SPA_POD_CHOICE_RANGE_Int(4, 2, 8),
            SPA_PARAM_BUFFERS_blocks, SPA_POD_Int(1),
            SPA_PARAM_BUFFERS_size, SPA_POD_Int(stride * f->info.size.height + 8),
            SPA_PARAM_BUFFERS_stride, SPA_POD_Int(stride),
            SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int((1 << SPA_DATA_MemFd))),
        spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
            SPA_PARAM_META_type, SPA_POD_Id(SPA_META_Header), SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_header))),
        spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
            SPA_PARAM_META_type, SPA_POD_Id(SPA_META_VideoCrop), SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_region))),
        spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
            SPA_PARAM_META_type, SPA_POD_Id(SPA_META_VideoTransform), SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_videotransform))),
    };
    pw_stream_update_params(f->stream, params, 4);
}
static void quit(void *userdata, int sig) { (void)sig; pw_main_loop_quit(((struct fixture *)userdata)->loop); }
static void rotate(void *userdata, int sig) { (void)sig; ((struct fixture *)userdata)->transform = (((struct fixture *)userdata)->transform + 1) % 8; }
static void resize(void *userdata, int sig) {
    (void)sig;
    struct fixture *f = userdata;
    uint8_t bytes[1024]; struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(bytes, sizeof(bytes));
    const struct spa_pod *params[] = { spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video), SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_Id(SPA_VIDEO_FORMAT_BGRx),
        SPA_FORMAT_VIDEO_size, SPA_POD_Rectangle(&SPA_RECTANGLE(48,32)),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_Fraction(&SPA_FRACTION(25,1))) };
    pw_stream_update_params(f->stream, params, 1);
}
int main(int argc, char **argv) {
    /* Block before PipeWire creates threads, so the loop's signalfd owns these
     * test commands instead of SIGUSR1 terminating an unblocked data thread. */
    sigset_t signals; sigemptyset(&signals); sigaddset(&signals, SIGTERM); sigaddset(&signals, SIGUSR1); sigaddset(&signals, SIGUSR2);
    if (sigprocmask(SIG_BLOCK, &signals, NULL) < 0) return 5;
    pw_init(&argc, &argv);
    struct fixture f = {0};
    f.loop = pw_main_loop_new(NULL);
    if (!f.loop) return 2;
    struct pw_context *context = pw_context_new(pw_main_loop_get_loop(f.loop), NULL, 0);
    struct pw_core *core = pw_context_connect(context, NULL, 0);
    if (!core) return 3;
    f.stream = pw_stream_new(core, "owned-pixels", pw_properties_new(PW_KEY_NODE_NAME, "owned-cu-source",
        PW_KEY_MEDIA_CLASS, "Video/Source", "node.supports-request", "1", NULL));
    f.timer = pw_loop_add_timer(pw_main_loop_get_loop(f.loop), tick, &f);
    static const struct pw_stream_events events = { PW_VERSION_STREAM_EVENTS, .state_changed=state, .param_changed=format, .process=process };
    struct spa_hook listener; pw_stream_add_listener(f.stream, &listener, &events, &f);
    struct spa_source *sig = pw_loop_add_signal(pw_main_loop_get_loop(f.loop), SIGTERM, quit, &f);
    struct spa_source *change = pw_loop_add_signal(pw_main_loop_get_loop(f.loop), SIGUSR1, resize, &f);
    struct spa_source *rotation = pw_loop_add_signal(pw_main_loop_get_loop(f.loop), SIGUSR2, rotate, &f);
    struct spa_source *commands = pw_loop_add_io(pw_main_loop_get_loop(f.loop), STDIN_FILENO, SPA_IO_IN, false, command, &f);
    uint8_t bytes[1024]; struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(bytes, sizeof(bytes));
    const struct spa_pod *params[] = { spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video), SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_Id(SPA_VIDEO_FORMAT_BGRx),
        SPA_FORMAT_VIDEO_size, SPA_POD_Rectangle(&SPA_RECTANGLE(32,24)),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_Fraction(&SPA_FRACTION(25,1))) };
    if (pw_stream_connect(f.stream, PW_DIRECTION_OUTPUT, PW_ID_ANY, PW_STREAM_FLAG_DRIVER | PW_STREAM_FLAG_MAP_BUFFERS, params, 1) < 0) return 4;
    pw_main_loop_run(f.loop);
    pw_loop_destroy_source(pw_main_loop_get_loop(f.loop), sig);
    pw_loop_destroy_source(pw_main_loop_get_loop(f.loop), change);
    pw_loop_destroy_source(pw_main_loop_get_loop(f.loop), rotation);
    pw_loop_destroy_source(pw_main_loop_get_loop(f.loop), commands);
    pw_loop_destroy_source(pw_main_loop_get_loop(f.loop), f.timer);
    spa_hook_remove(&listener); pw_stream_destroy(f.stream); pw_core_disconnect(core);
    pw_context_destroy(context); pw_main_loop_destroy(f.loop);
    return 0;
}
