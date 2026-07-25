#include <stdint.h>

void *matrix_saver_new(void *ns_view, uint32_t width, uint32_t height, const char *version, uint8_t mirror_enabled, uint8_t skip_intro);
void matrix_saver_apply_settings(void *state, const char *version, uint8_t mirror_enabled, uint8_t skip_intro);
void matrix_saver_resize(void *state, uint32_t width, uint32_t height);
void matrix_saver_render(void *state);
void matrix_saver_free(void *state);
