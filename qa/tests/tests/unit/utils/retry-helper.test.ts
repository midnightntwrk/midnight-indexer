// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// `retry` has one deliberate exception to "try again until the budget runs
// out": a `NonRetryableError` is re-thrown at once. That is what lets a poll
// against a dead websocket fail in one attempt instead of burning minutes and
// then reporting only "not found yet". These tests pin the exception down, and
// pin down the ordinary behaviour next to it so the difference is visible.

import log from '@utils/logging/logger';
import { NonRetryableError, retry } from '@utils/retry-helper';

// The real logger creates a log directory on import and writes a warning on
// every retry; the mock keeps the test hermetic and lets the warning be
// asserted absent.
vi.mock('@utils/logging/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));

/** Real timers, but no reason to actually wait between attempts. */
const FAST = { delayMs: 1 };

describe('retry', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  /**
   * @given a function that succeeds on its first call
   * @when it is wrapped in retry
   * @then its result is returned after a single call
   */
  test('should return the result of the first successful attempt', async () => {
    const fn = vi.fn().mockResolvedValue('ok');

    await expect(retry(fn, { maxRetries: 3, ...FAST })).resolves.toBe('ok');
    expect(fn).toHaveBeenCalledTimes(1);
    expect(log.warn).not.toHaveBeenCalled();
  });

  /**
   * @given a function that keeps failing with an ordinary Error
   * @when it is wrapped in retry with a budget of two extra attempts
   * @then it is called three times and the final error wraps the last failure
   */
  test('should spend the whole budget on an ordinary error', async () => {
    const lastError = new Error('still not found');
    const fn = vi.fn().mockRejectedValue(lastError);

    await expect(retry(fn, { maxRetries: 2, retryLabel: 'poll', ...FAST })).rejects.toMatchObject({
      message: 'Failed after 3 attempts for poll. Last error: still not found',
      cause: lastError,
    });
    expect(fn).toHaveBeenCalledTimes(3);
    expect(log.warn).toHaveBeenCalledTimes(2);
  });

  /**
   * @given a function that fails with a NonRetryableError
   * @when it is wrapped in retry with a generous budget
   * @then it is called exactly once and the very same error surfaces, unwrapped
   */
  test('should re-throw a NonRetryableError after exactly one attempt', async () => {
    const error = new NonRetryableError('the socket is gone');
    const fn = vi.fn().mockRejectedValue(error);

    await expect(retry(fn, { maxRetries: 5, retryLabel: 'poll', ...FAST })).rejects.toBe(error);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(log.warn).not.toHaveBeenCalled();
  });

  /**
   * @given a subclass of NonRetryableError, as DeadSocketError is
   * @when it is thrown from a retried function
   * @then it is treated the same way: one attempt, same instance
   */
  test('should treat a subclass of NonRetryableError as non-retryable', async () => {
    class SocketGoneError extends NonRetryableError {}
    const error = new SocketGoneError('bare 1006');
    const fn = vi.fn().mockRejectedValue(error);

    await expect(retry(fn, { maxRetries: 5, ...FAST })).rejects.toBe(error);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  /**
   * @given a function that fails ordinarily first and then non-retryably
   * @when it is wrapped in retry with attempts still left in the budget
   * @then the remaining attempts are abandoned the moment the terminal error appears
   */
  test('should stop mid-budget when a NonRetryableError appears', async () => {
    const terminal = new NonRetryableError('the socket died meanwhile');
    const fn = vi
      .fn()
      .mockRejectedValueOnce(new Error('not found yet'))
      .mockRejectedValueOnce(terminal);

    await expect(retry(fn, { maxRetries: 10, ...FAST })).rejects.toBe(terminal);
    expect(fn).toHaveBeenCalledTimes(2);
    expect(log.warn).toHaveBeenCalledTimes(1);
  });
});
