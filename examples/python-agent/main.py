"""使用 Python 异步客户端执行一个版本化 Recall Fixture。"""

import asyncio
import json
import os
from pathlib import Path

from jiangcheng_memory import AsyncRemoteMemoryClient


async def main() -> None:
    fixture_path = Path(__file__).parents[2] / "sdk" / "contracts" / "fixtures" / "recall-exact.json"
    fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
    async with AsyncRemoteMemoryClient(
        base_url=os.environ.get("JC_MEMORY_URL", "http://127.0.0.1:8080"),
        api_key=os.environ.get("JC_MEMORY_API_KEY"),
    ) as client:
        response = await client.recall(fixture["request"])
        print(json.dumps(response, ensure_ascii=False))


if __name__ == "__main__":
    asyncio.run(main())
