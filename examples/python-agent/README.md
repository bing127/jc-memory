# Python Agent 示例

示例只从环境变量读取服务地址和密钥：

```bash
export JC_MEMORY_URL=http://127.0.0.1:8080
export JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)"
.venv/bin/python examples/python-agent/main.py
```
