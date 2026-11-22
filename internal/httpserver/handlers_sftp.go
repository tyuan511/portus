package httpserver

import (
	"bufio"
	"io"
	"mime"
	"net/url"
	"os"
	"path"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/pkg/sftp"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
	"github.com/yuantang/sshield/internal/metrics"
	"go.uber.org/zap"
)

// registerSFTPRoutes mounts the browser file manager: browse/upload/download/
// mkdir/rename/delete over the SFTP subsystem of a host. Every operation
// requires the host.transfer permission plus a can_transfer grant.
func (s *Server) registerSFTPRoutes(g fiber.Router) {
	g.Get("/servers/:id/files", s.handleSFTPList)
	g.Get("/servers/:id/files/download", s.handleSFTPDownload)
	g.Post("/servers/:id/files/upload", s.handleSFTPUpload)
	g.Post("/servers/:id/files/mkdir", s.handleSFTPMkdir)
	g.Post("/servers/:id/files/delete", s.handleSFTPDelete)
	g.Post("/servers/:id/files/rename", s.handleSFTPRename)
}

// sftpAsset resolves the host and verifies transfer authorization.
func (s *Server) sftpAsset(c *fiber.Ctx) (*domain.ServerAsset, error) {
	id := identityFrom(c)
	asset, err := s.db.GetServer(c.Params("id"))
	if err != nil {
		return nil, apierr.NotFoundf("server not found")
	}
	if !asset.Enabled {
		return nil, apierr.Forbidden("server is disabled")
	}
	if err := s.rbac.CheckServerTransfer(id, asset); err != nil {
		return nil, err
	}
	return asset, nil
}

// sftpClient opens an SFTP client for the asset with the jump chain resolved.
func (s *Server) sftpClient(asset *domain.ServerAsset) (*sftp.Client, error) {
	all, err := s.db.ListServers()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return s.pool.OpenSFTP(asset, assetMap(all))
}

// sftpEvent writes the audit entry for a file operation.
func (s *Server) sftpEvent(c *fiber.Ctx, asset *domain.ServerAsset, op, detail string, failed bool, extra string) {
	decision := domain.DecisionAllow
	if failed {
		decision = domain.DecisionDeny
	}
	id := identityFrom(c)
	ev := &domain.AuditEvent{
		ID: newAuditID(), Actor: "sftp", Timestamp: time.Now().UTC(),
		ServerID: asset.ID, ServerName: asset.Name,
		SSHUser: asset.Username, ServerHost: hostPort(asset),
		Command: op, Intent: detail, Decision: decision, Error: extra,
	}
	if id != nil {
		ev.UserID = id.UserID
		ev.Username = id.Username
	}
	s.Audit(ev)
	if failed {
		recordDenialMetrics("sftp")
	}
}

type sftpEntry struct {
	Name    string  `json:"name"`
	Path    string  `json:"path"`
	IsDir   bool    `json:"is_dir"`
	Size    int64   `json:"size"`
	Mode    string  `json:"mode"`
	ModTime string  `json:"mod_time"`
	Target  *string `json:"target,omitempty"`
}

func (s *Server) handleSFTPList(c *fiber.Ctx) error {
	asset, err := s.sftpAsset(c)
	if err != nil {
		return err
	}
	dir := c.Query("path", ".")
	sc, err := s.sftpClient(asset)
	if err != nil {
		return err
	}
	defer sc.Close()

	entries, err := sc.ReadDir(dir)
	if err != nil {
		s.sftpEvent(c, asset, "file.list "+dir, "SFTP browse", true, err.Error())
		return apierr.Wrap(apierr.CodeSSHError, 502, "readdir failed", err)
	}
	out := make([]sftpEntry, 0, len(entries))
	for _, e := range entries {
		entry := sftpEntry{
			Name:    e.Name(),
			Path:    path.Join(dir, e.Name()),
			IsDir:   e.IsDir(),
			Size:    e.Size(),
			Mode:    e.Mode().String(),
			ModTime: e.ModTime().UTC().Format(time.RFC3339),
		}
		if e.Mode()&os.ModeSymlink != 0 {
			if target, err := sc.ReadLink(path.Join(dir, e.Name())); err == nil {
				entry.Target = &target
			}
		}
		out = append(out, entry)
	}
	s.sftpEvent(c, asset, "file.list "+dir, "SFTP browse", false, "")
	return resp.OK(c, fiber.Map{"path": dir, "entries": out})
}

func (s *Server) handleSFTPDownload(c *fiber.Ctx) error {
	asset, err := s.sftpAsset(c)
	if err != nil {
		return err
	}
	filePath := c.Query("path")
	if filePath == "" {
		return apierr.BadRequest("path is required")
	}
	sc, err := s.sftpClient(asset)
	if err != nil {
		return err
	}

	info, err := sc.Stat(filePath)
	if err != nil {
		sc.Close()
		s.sftpEvent(c, asset, "file.download "+filePath, "SFTP download", true, err.Error())
		return apierr.Wrap(apierr.CodeSSHError, 502, "stat failed", err)
	}
	if info.IsDir() {
		sc.Close()
		return apierr.Unprocessable("cannot download a directory")
	}
	f, err := sc.Open(filePath)
	if err != nil {
		sc.Close()
		s.sftpEvent(c, asset, "file.download "+filePath, "SFTP download", true, err.Error())
		return apierr.Wrap(apierr.CodeSSHError, 502, "open failed", err)
	}

	s.sftpEvent(c, asset, "file.download "+filePath, "SFTP download", false, "")
	metrics.FileOpsTotal.WithLabelValues("download").Inc()
	// The response body is written after the handler returns, so the SFTP
	// client and file must stay open until the stream writer finishes —
	// closing them here would kill the reader mid-transfer.
	c.Set("Content-Type", mime.TypeByExtension(path.Ext(filePath)))
	c.Set("Content-Disposition", "attachment; filename*=UTF-8''"+url.PathEscape(info.Name()))
	c.Context().Response.Header.SetContentLength(int(info.Size()))
	c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
		defer f.Close()
		defer sc.Close()
		buf := make([]byte, 32*1024)
		var written int64
		for written < info.Size() {
			n, err := f.ReadAt(buf, written)
			if n > 0 {
				if _, werr := w.Write(buf[:n]); werr != nil {
					s.log.Warn("sftp download write failed", zap.String("path", filePath), zap.Error(werr))
					return
				}
				written += int64(n)
			}
			if err != nil {
				s.log.Warn("sftp download read failed",
					zap.String("path", filePath), zap.Int64("written", written), zap.Error(err))
				return // EOF or read error: nothing more to stream
			}
		}
	})
	return nil
}

func (s *Server) handleSFTPUpload(c *fiber.Ctx) error {
	asset, err := s.sftpAsset(c)
	if err != nil {
		return err
	}
	form, err := c.MultipartForm()
	if err != nil {
		return apierr.BadRequest("multipart form required")
	}
	dir := form.Value["path"]
	if len(dir) == 0 || dir[0] == "" {
		return apierr.Unprocessable("path is required")
	}
	files := form.File["files"]
	if len(files) == 0 {
		files = form.File["files[]"]
	}
	if len(files) == 0 {
		return apierr.Unprocessable("files are required")
	}
	sc, err := s.sftpClient(asset)
	if err != nil {
		return err
	}
	defer sc.Close()

	results := make([]fiber.Map, 0, len(files))
	failed := false
	for _, fh := range files {
		dst := path.Join(dir[0], fh.Filename)
		src, err := fh.Open()
		if err != nil {
			failed = true
			results = append(results, fiber.Map{"file": fh.Filename, "error": err.Error()})
			continue
		}
		dstFile, err := sc.Create(dst)
		if err != nil {
			_ = src.Close()
			failed = true
			s.sftpEvent(c, asset, "file.upload "+dst, "SFTP upload", true, err.Error())
			results = append(results, fiber.Map{"file": fh.Filename, "error": err.Error()})
			continue
		}
		written, err := io.Copy(dstFile, src)
		closeErr := dstFile.Close()
		_ = src.Close()
		if err != nil || closeErr != nil {
			failed = true
			s.sftpEvent(c, asset, "file.upload "+dst, "SFTP upload", true, firstErr(err, closeErr))
			results = append(results, fiber.Map{"file": fh.Filename, "error": firstErr(err, closeErr)})
			continue
		}
		s.sftpEvent(c, asset, "file.upload "+dst, "SFTP upload", false, "")
		metrics.FileOpsTotal.WithLabelValues("upload").Inc()
		results = append(results, fiber.Map{"file": fh.Filename, "path": dst, "bytes": written})
	}
	s.notify.Send("file.uploaded",
		"Files uploaded to "+asset.Name,
		map[string]any{"server": asset.Name, "user": identityFrom(c).Username, "count": len(files)})
	return resp.OK(c, fiber.Map{"results": results, "failed": failed})
}

func (s *Server) handleSFTPMkdir(c *fiber.Ctx) error {
	asset, err := s.sftpAsset(c)
	if err != nil {
		return err
	}
	var req struct {
		Path string `json:"path"`
	}
	if err := c.BodyParser(&req); err != nil || req.Path == "" {
		return apierr.Unprocessable("path is required")
	}
	sc, err := s.sftpClient(asset)
	if err != nil {
		return err
	}
	defer sc.Close()

	if err := sc.MkdirAll(req.Path); err != nil {
		s.sftpEvent(c, asset, "file.mkdir "+req.Path, "SFTP mkdir", true, err.Error())
		return apierr.Wrap(apierr.CodeSSHError, 502, "mkdir failed", err)
	}
	s.sftpEvent(c, asset, "file.mkdir "+req.Path, "SFTP mkdir", false, "")
	metrics.FileOpsTotal.WithLabelValues("mkdir").Inc()
	return resp.Created(c, fiber.Map{"path": req.Path})
}

func (s *Server) handleSFTPDelete(c *fiber.Ctx) error {
	asset, err := s.sftpAsset(c)
	if err != nil {
		return err
	}
	var req struct {
		Paths []string `json:"paths"`
	}
	if err := c.BodyParser(&req); err != nil || len(req.Paths) == 0 {
		return apierr.Unprocessable("paths is required")
	}
	sc, err := s.sftpClient(asset)
	if err != nil {
		return err
	}
	defer sc.Close()

	results := make([]fiber.Map, 0, len(req.Paths))
	failed := false
	for _, p := range req.Paths {
		if err := s.sftpRemoveAll(sc, p); err != nil {
			failed = true
			s.sftpEvent(c, asset, "file.delete "+p, "SFTP delete", true, err.Error())
			results = append(results, fiber.Map{"path": p, "error": err.Error()})
			continue
		}
		s.sftpEvent(c, asset, "file.delete "+p, "SFTP delete", false, "")
		metrics.FileOpsTotal.WithLabelValues("delete").Inc()
		results = append(results, fiber.Map{"path": p, "deleted": true})
	}
	return resp.OK(c, fiber.Map{"results": results, "failed": failed})
}

// sftpRemoveAll deletes a file or recursively a directory tree.
func (s *Server) sftpRemoveAll(sc *sftp.Client, p string) error {
	info, err := sc.Stat(p)
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return sc.Remove(p)
	}
	entries, err := sc.ReadDir(p)
	if err != nil {
		return err
	}
	for _, e := range entries {
		if err := s.sftpRemoveAll(sc, path.Join(p, e.Name())); err != nil {
			return err
		}
	}
	return sc.RemoveDirectory(p)
}

func (s *Server) handleSFTPRename(c *fiber.Ctx) error {
	asset, err := s.sftpAsset(c)
	if err != nil {
		return err
	}
	var req struct {
		From string `json:"from"`
		To   string `json:"to"`
	}
	if err := c.BodyParser(&req); err != nil || req.From == "" || req.To == "" {
		return apierr.Unprocessable("from and to are required")
	}
	sc, err := s.sftpClient(asset)
	if err != nil {
		return err
	}
	defer sc.Close()

	if err := sc.Rename(req.From, req.To); err != nil {
		s.sftpEvent(c, asset, "file.rename "+req.From+" -> "+req.To, "SFTP rename", true, err.Error())
		return apierr.Wrap(apierr.CodeSSHError, 502, "rename failed", err)
	}
	s.sftpEvent(c, asset, "file.rename "+req.From+" -> "+req.To, "SFTP rename", false, "")
	metrics.FileOpsTotal.WithLabelValues("rename").Inc()
	return resp.OK(c, fiber.Map{"from": req.From, "to": req.To})
}

func firstErr(errs ...error) string {
	for _, e := range errs {
		if e != nil {
			return e.Error()
		}
	}
	return ""
}
