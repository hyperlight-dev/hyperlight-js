import { defineConfig } from 'vitest/config';

export default defineConfig({
    test: {
        // Test files pattern
        include: ['tests/**/*.test.js'],
        // Generous timeout — sandbox builds can be slow, and timeout tests
        // need time to actually time out
        testTimeout: 30000,
        // Hook timeout — beforeAll spawns the server and initializes the
        // sandbox which involves compiling the QuickJS runtime
        hookTimeout: 60000,
    },
});
