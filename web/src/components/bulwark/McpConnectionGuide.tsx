import { useState } from "react";
import { Check, Copy, Plug } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useI18n } from "@/lib/i18n";

interface McpConnectionGuideProps {
  /** Plaintext token when just created; falls back to placeholder otherwise. */
  token?: string;
}

function CopyButton({ text, label }: { text: string; label?: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      /* clipboard may be unavailable in non-secure contexts */
    }
  };

  return (
    <Button
      type="button"
      variant="outline"
      size="xs"
      onClick={() => void handleCopy()}
      className="gap-1.5"
    >
      {copied ? <Check className="size-3 text-emerald-500" /> : <Copy className="size-3" />}
      {copied ? t("settings.mcpCopied") : (label ?? t("common.copy"))}
    </Button>
  );
}

/**
 * Persistent MCP client connection guide shown in Settings -> API Tokens.
 * Explains endpoint URL, auth header, client config snippets, and host session workflow.
 */
export function McpConnectionGuide({ token }: McpConnectionGuideProps) {
  const { t, lang } = useI18n();
  const origin = typeof window !== "undefined" ? window.location.origin : "http://localhost:8787";
  const endpointUrl = `${origin}/mcp`;
  const effectiveToken = token || "sk-bulwark-xxxxxxxx";

  const jsonConfig = JSON.stringify(
    {
      mcpServers: {
        bulwark: {
          type: "http",
          url: endpointUrl,
          headers: {
            Authorization: `Bearer ${effectiveToken}`,
          },
        },
      },
    },
    null,
    2,
  );

  const cliCommand = `claude mcp add --transport http bulwark ${endpointUrl} --header "Authorization: Bearer ${effectiveToken}"`;

  const codexConfig = [
    `# ~/.codex/config.toml`,
    `[mcp_servers.bulwark]`,
    `url = "${endpointUrl}"`,
    ``,
    `[mcp_servers.bulwark.http_headers]`,
    `Authorization = "Bearer ${effectiveToken}"`,
  ].join("\n");

  const agentPrompt =
    lang === "zh-CN"
      ? [
          `请帮我将 Bulwark MCP 服务器添加到当前工具的 MCP 配置中，连接信息如下：`,
          `- 名称：bulwark`,
          `- 传输协议：Streamable HTTP (http)`,
          `- URL：${endpointUrl}`,
          `- 请求头：Authorization: Bearer ${effectiveToken}`,
          `- Content-Type：application/json`,
          `- Accept：application/json, text/event-stream`,
          ``,
          `添加完成后，请列出可用的 MCP 工具以确认连接成功。`,
        ].join("\n")
      : [
          `Please add the Bulwark MCP server to my current tool's MCP configuration using these connection details:`,
          `- Name: bulwark`,
          `- Transport: Streamable HTTP (http)`,
          `- URL: ${endpointUrl}`,
          `- Header: Authorization: Bearer ${effectiveToken}`,
          `- Content-Type: application/json`,
          `- Accept: application/json, text/event-stream`,
          ``,
          `After adding it, list the available MCP tools to verify the connection.`,
        ].join("\n");

  const curlCommand = [
    `curl -X POST ${endpointUrl} \\`,
    `  -H "Authorization: Bearer ${effectiveToken}" \\`,
    `  -H "Content-Type: application/json" \\`,
    `  -H "Accept: application/json, text/event-stream" \\`,
    `  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'`,
  ].join("\n");

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Plug className="size-4 text-muted-foreground" />
            <CardTitle className="text-base">{t("settings.mcpGuideTitle")}</CardTitle>
          </div>
          <Badge variant="secondary" className="font-mono text-[11px]">
            {t("settings.mcpTransport")}: Streamable HTTP
          </Badge>
        </div>
        <CardDescription>{t("settings.mcpGuideDesc")}</CardDescription>
      </CardHeader>

      <CardContent className="flex flex-col gap-5">
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border bg-secondary/40 px-3 py-2">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="text-xs font-medium text-muted-foreground">
              {t("settings.mcpEndpointLabel")}:
            </span>
            <code className="break-all font-mono text-xs text-foreground">{endpointUrl}</code>
          </div>
          <CopyButton text={endpointUrl} />
        </div>

        <div className="grid gap-3 md:grid-cols-3">
          <div className="rounded-lg border border-border/80 bg-background/60 p-3">
            <div className="text-xs font-semibold text-foreground">{t("settings.mcpStep1Title")}</div>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              {t("settings.mcpStep1Desc")}
            </p>
          </div>
          <div className="rounded-lg border border-border/80 bg-background/60 p-3">
            <div className="text-xs font-semibold text-foreground">{t("settings.mcpStep2Title")}</div>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              {t("settings.mcpStep2Desc")}
            </p>
          </div>
          <div className="rounded-lg border border-border/80 bg-background/60 p-3">
            <div className="text-xs font-semibold text-foreground">{t("settings.mcpStep3Title")}</div>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              {t("settings.mcpStep3Desc")}
            </p>
          </div>
        </div>

        <Tabs defaultValue="json" className="gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <TabsList className="flex-wrap">
              <TabsTrigger value="json">{t("settings.mcpTabJson")}</TabsTrigger>
              <TabsTrigger value="cli">{t("settings.mcpTabCli")}</TabsTrigger>
              <TabsTrigger value="codex">{t("settings.mcpTabCodex")}</TabsTrigger>
              <TabsTrigger value="prompt">{t("settings.mcpTabPrompt")}</TabsTrigger>
              <TabsTrigger value="curl">{t("settings.mcpTabCurl")}</TabsTrigger>
            </TabsList>
            {token && (
              <span className="text-xs text-emerald-600 dark:text-emerald-400">
                {t("settings.mcpTokenInjected")}
              </span>
            )}
          </div>

          <TabsContent value="json" className="relative">
            <div className="absolute top-2 right-2">
              <CopyButton text={jsonConfig} />
            </div>
            <pre className="overflow-x-auto rounded-lg border border-border bg-secondary/50 p-3 pr-20 font-mono text-xs leading-relaxed text-foreground">
              <code>{jsonConfig}</code>
            </pre>
          </TabsContent>

          <TabsContent value="cli" className="relative">
            <div className="absolute top-2 right-2">
              <CopyButton text={cliCommand} />
            </div>
            <pre className="overflow-x-auto rounded-lg border border-border bg-secondary/50 p-3 pr-20 font-mono text-xs leading-relaxed text-foreground">
              <code>{cliCommand}</code>
            </pre>
          </TabsContent>

          <TabsContent value="codex" className="relative">
            <div className="absolute top-2 right-2">
              <CopyButton text={codexConfig} />
            </div>
            <pre className="overflow-x-auto rounded-lg border border-border bg-secondary/50 p-3 pr-20 font-mono text-xs leading-relaxed text-foreground">
              <code>{codexConfig}</code>
            </pre>
          </TabsContent>

          <TabsContent value="prompt" className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">{t("settings.mcpPromptHint")}</p>
            <div className="relative">
              <div className="absolute top-2 right-2">
                <CopyButton text={agentPrompt} />
              </div>
              <pre className="overflow-x-auto whitespace-pre-wrap rounded-lg border border-border bg-secondary/50 p-3 pr-20 font-mono text-xs leading-relaxed text-foreground">
                <code>{agentPrompt}</code>
              </pre>
            </div>
          </TabsContent>

          <TabsContent value="curl" className="relative">
            <div className="absolute top-2 right-2">
              <CopyButton text={curlCommand} />
            </div>
            <pre className="overflow-x-auto rounded-lg border border-border bg-secondary/50 p-3 pr-20 font-mono text-xs leading-relaxed text-foreground">
              <code>{curlCommand}</code>
            </pre>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  );
}
