package com.jiangcheng.memory.android

import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean
import uniffi.jc_memory_uniffi.JcMemoryEngine

/** 通过 Android jniLibs 中的 UniFFI native slice 调用 Rust Core。 */
class AndroidEmbeddedMemoryClient(
    dataDirectory: String,
    principalId: String,
) : Closeable {
    private val closed = AtomicBoolean(false)
    private val engine: JcMemoryEngine

    init {
        // jniLibs 由发布 AAR 提供；未打包 slice 时让系统尽早报告配置错误。
        System.loadLibrary("jc_memory_uniffi")
        engine = JcMemoryEngine(dataDirectory, principalId)
    }

    /** 执行 JSON Request Envelope。 */
    @Synchronized
    fun execute(requestJson: String): String {
        check(!closed.get()) { "Android Embedded 客户端已经关闭" }
        return engine.execute(requestJson)
    }

    /** 关闭 Rust handle；重复关闭保持幂等。 */
    @Synchronized
    override fun close() {
        if (closed.compareAndSet(false, true)) {
            engine.shutdown()
            engine.close()
        }
    }
}
