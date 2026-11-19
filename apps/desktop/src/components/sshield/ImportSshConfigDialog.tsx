import { useState } from "react";
import { FolderInput } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";

interface ImportSshConfigDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImport: (content?: string) => Promise<void>;
}

export function ImportSshConfigDialog({
  open,
  onOpenChange,
  onImport,
}: ImportSshConfigDialogProps) {
  const [paste, setPaste] = useState("");

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-white/10 bg-[#111622] text-slate-100 sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>导入 OpenSSH Config</DialogTitle>
          <DialogDescription>
            直接读取本机 ~/.ssh/config，或粘贴配置文本以自动解析主机与 ProxyJump 拓扑。
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Button
            className="w-full bg-indigo-600 text-white hover:bg-indigo-500"
            onClick={() => onImport(undefined)}
          >
            <FolderInput className="mr-1.5 size-4" /> 一键读取 ~/.ssh/config
          </Button>
          <Separator className="bg-white/10" />
          <Label>或粘贴 ~/.ssh/config 内容：</Label>
          <Textarea
            rows={6}
            placeholder={"Host prod-db\n  HostName 10.0.2.15\n  User ubuntu\n  ProxyJump bastion"}
            value={paste}
            onChange={(e) => setPaste(e.target.value)}
            className="font-mono text-xs"
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button
            disabled={!paste.trim()}
            onClick={async () => {
              await onImport(paste);
              setPaste("");
            }}
          >
            解析粘贴内容
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
