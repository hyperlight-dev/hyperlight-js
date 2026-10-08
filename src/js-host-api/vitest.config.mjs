import { defineConfig } from 'vitest/config';

export default defineConfig({
    test: {
        // Hyperlight sandboxes use hardware virtualization and must not compete across workers.
        fileParallelism: false,
        // Test files pattern
        include: ['tests/**/*.test.js'],
        // Increase timeout for sandbox operations (some involve busy loops)
        testTimeout: 30000,
    },
});
