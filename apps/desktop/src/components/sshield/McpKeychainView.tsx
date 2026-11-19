import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

const MCP_SNIPPET = JSON.stringify(
  {
    mcpServers: {
      sshield: {
        command: "sshield-server",
        args: ["--mcp-stdio"],
      },
    },
  },
  null,
  2
);

export function McpKeychainView() {
  const [copied, setCopied] = useState(false);

  return (
    <div className="flex-1 space-y-6 overflow-y-auto p-6">
      <div>
        <h2 className="text-xl font-semibold text-foreground">MCP 接入与系统密钥链</h2>
        <p className="text-xs text-muted-foreground">
          在 Cursor / Claude Desktop 中配置 stdio MCP Server，共享同一本地 SQLite 资产与规则库。
        </p>
      </div>

      <Card className="border-border bg-card">
        <CardHeader className="flex flex-row items-center justify-between">
          <div>
            <CardTitle className="text-sm">Cursor / Claude Desktop MCP 配置</CardTitle>
            <CardDescription>
              暴露 `list_authorized_servers` 与 `execute_command` 工具
            </CardDescription>
          </div>
          <Button
            size="sm"
            variant="outline"
            className="border-border"
            onClick={() => {
              void navigator.clipboard.writeText(MCP_SNIPPET);
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            }}
          >
            {copied ? (
              <>
                <Check className="mr-1.5 size-3.5 text-emerald-400" /> 已复制
              </>
            ) : (
              <>
                <Copy className="mr-1.5 size-3.5" /> 复制 JSON
              </>
            )}
          </Button>
        </CardHeader>
        <CardContent>
          <pre className="overflow-x-auto rounded-lg bg-muted/60 p-4 font-mono text-xs text-indigo-300">
            {MCP_SNIPPET}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}
