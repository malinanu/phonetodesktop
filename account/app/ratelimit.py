"""A small sliding-window limiter, kept in memory.

It protects each running process. With several replicas behind a load balancer each keeps its own count, so the
effective limit is multiplied; put a real limiter (Caddy/nginx or a shared store) in front for that setup.
"""
from __future__ import annotations

import time
from collections import defaultdict, deque

from fastapi import HTTPException


class RateLimiter:
    def __init__(self, limit: int, window_s: int, clock=time.monotonic) -> None:
        self.limit, self.window, self.clock = limit, window_s, clock
        self._hits: dict[str, deque[float]] = defaultdict(deque)

    def check(self, key: str) -> None:
        t = self.clock()
        q = self._hits[key]
        while q and q[0] <= t - self.window:
            q.popleft()
        if len(q) >= self.limit:
            raise HTTPException(429, "Too many requests. Try again in a few minutes.", headers={"Retry-After": str(self.window)})
        q.append(t)
        if len(self._hits) > 50_000:  # never let an attacker grow this without bound
            for k in [k for k, v in self._hits.items() if not v or v[-1] <= t - self.window]:
                self._hits.pop(k, None)
