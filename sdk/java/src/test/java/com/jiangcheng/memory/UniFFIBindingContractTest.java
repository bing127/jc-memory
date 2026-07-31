package com.jiangcheng.memory;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;

class UniFFIBindingContractTest {
    @Test
    void generatedBindingExposesEngineType() {
        assertDoesNotThrow(() -> {
            var type = Class.forName("uniffi.jc_memory_uniffi.JcMemoryEngine");
            type.getMethod("execute", String.class);
            type.getMethod("shutdown");
        });
    }
}
