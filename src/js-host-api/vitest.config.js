import { defineConfig } from 'vitest/config';

export default defineConfig({
    test: {
        // Test files pattern
        include: ['tests/**/*.test.js'],
        // Windows ARM64 WHP currently supports only 64 concurrent partitions.
        // Hyperlight's permit counter is process-local, so separate Vitest workers
        // cannot coordinate that limit. Run one file at a time on this host.
        ...(process.platform === 'win32' && process.arch === 'arm64'
            ? { fileParallelism: false, maxWorkers: 1 }
            : {}),
        // Increase timeout for sandbox operations (some involve busy loops)
        testTimeout: 30000,
    },
});
