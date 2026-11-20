// Package resp renders the unified API envelope and integrates with Fiber's
// centralized error handler.
package resp

import (
	"errors"

	"github.com/gofiber/fiber/v2"
	"go.uber.org/zap"

	"github.com/yuantang/sshield/internal/apierr"
)

// Envelope is the wire format of every /api JSON response.
type Envelope struct {
	Code    apierr.Code `json:"code"`
	Message string      `json:"message"`
	Data    any         `json:"data,omitempty"`
	TraceID string      `json:"trace_id,omitempty"`
}

// traceID extracts the request id injected by the requestid middleware.
func traceID(c *fiber.Ctx) string {
	if v, ok := c.Locals("requestid").(string); ok {
		return v
	}
	return ""
}

// OK writes a success envelope. Handlers "return OK(c, data)" so the Fiber
// error handler stays the single failure path.
func OK(c *fiber.Ctx, data any) error {
	return c.JSON(Envelope{Code: apierr.CodeOK, Message: "ok", Data: data, TraceID: traceID(c)})
}

// Created writes a success envelope with HTTP 201.
func Created(c *fiber.Ctx, data any) error {
	c.Status(fiber.StatusCreated)
	return c.JSON(Envelope{Code: apierr.CodeOK, Message: "created", Data: data, TraceID: traceID(c)})
}

// Fail converts err into the error envelope, preserving structured codes and
// logging unexpected internals.
func Fail(c *fiber.Ctx, err error, log *zap.Logger) error {
	e := apierr.From(err)
	status := e.Status
	if status == 0 {
		status = fiber.StatusInternalServerError
	}
	if e.Code == apierr.CodeInternal {
		log.Error("request failed",
			zap.String("trace_id", traceID(c)),
			zap.String("path", c.Path()),
			zap.Error(err))
	}
	return c.Status(status).JSON(Envelope{
		Code:    e.Code,
		Message: e.Message,
		TraceID: traceID(c),
	})
}

// ErrorHandler is the app-wide Fiber error handler. It wraps all failures
// (returned errors and panics recovered by middleware) in the envelope.
// Non-/api paths (embedded SPA assets) fall back to plain status codes.
func ErrorHandler(log *zap.Logger) fiber.ErrorHandler {
	return func(c *fiber.Ctx, err error) error {
		if len(c.Path()) >= 4 && c.Path()[:4] == "/api" {
			return Fail(c, err, log)
		}
		var fe *fiber.Error
		if errors.As(err, &fe) {
			return c.Status(fe.Code).SendString(fe.Message)
		}
		return fiber.ErrInternalServerError
	}
}
