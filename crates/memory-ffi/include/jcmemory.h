#ifndef JCMEMORY_H
#define JCMEMORY_H

#include <stddef.h>
#include <stdint.h>

/*
 * C ABI 边界约束：
 * - jcmemory_create 接收 UTF-8、NUL 结尾路径和主体标识，失败返回 NULL；
 * - jcmemory_execute 返回由 Rust 分配的 UTF-8 JSON，*out_len 是精确字节数；
 * - 调用方只能用同一个指针和长度调用 jcmemory_free_buffer，不得自行 free；
 * - destroy 之后不得再使用 handle；generation 只用于拒绝过期 cancel token；
 * - 所有函数在 panic 或无效指针时返回失败值，不把 Rust panic 穿过 ABI。
 */

typedef struct JcMemoryHandle JcMemoryHandle;

JcMemoryHandle *jcmemory_create(const char *data_dir, const char *principal_id);
void jcmemory_destroy(JcMemoryHandle *handle);
uint8_t *jcmemory_execute(JcMemoryHandle *handle, const uint8_t *request,
                          size_t request_len, size_t *out_len);
uint8_t *jcmemory_execute_with_token(JcMemoryHandle *handle, uint64_t token,
                                     const uint8_t *request, size_t request_len,
                                     size_t *out_len);
int jcmemory_cancel(JcMemoryHandle *handle, uint64_t token);
void jcmemory_free_buffer(uint8_t *buffer, size_t buffer_len);

#endif
