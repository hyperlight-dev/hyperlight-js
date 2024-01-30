import { defineConfig } from 'vitest/config';

export default defineConfig({
    test: {
        // Test files pattern
        include: ['tests/**/*.test.js'],
        // Increase timeout for sandbox operations (some involve busy loops)
        testTimeout: 30000,
    },
});
