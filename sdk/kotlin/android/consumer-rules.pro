# 保留 UniFFI 生成 binding 和 JNA 的反射入口。
-keep class uniffi.jc_memory_uniffi.** { *; }
-keep class com.sun.jna.** { *; }
-keep class com.jiangcheng.memory.android.** { *; }
