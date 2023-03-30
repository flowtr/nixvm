const std = @import("std");

// Nix language parser

pub const Error = error{
    ParseError,
};

pub const ErrorInfo = struct {
    line: usize,
    col: usize,
    msg: []const u8,
};

pub const Node = union {
    Identifier: struct {
        name: []const u8,
    },

    Integer: struct {
        value: i64,
    },

    Float: struct {
        value: f64,
    },

    String: struct {
        value: []const u8,
    },

    List: struct {
        items: []const *Node,
    },

    Set: struct {
        items: []const *Node,
    },

    Attr: struct {
        name: *Node,
        value: *Node,
    },

    Lambda: struct {
        params: []const *Node,
        body: *Node,
    },

    UnaryOp: struct {
        child: *Node,
    },

    BinaryOp: struct {
        left: *Node,
        right: *Node,
    },
};

pub const Parser = struct {
    // The source code
    source: []const u8,

    // The current position in the source code
    pos: usize,

    // The current line number
    line: usize,

    // The current column number
    col: usize,

    allocator: std.mem.Allocator,

    errors: std.ArrayList(ErrorInfo),

    pub fn init(allocator: std.mem.Allocator, source: []const u8) Parser {
        return Parser{
            .source = source,
            .pos = 0,
            .line = 1,
            .col = 1,
            .allocator = allocator,
            .errors = std.ArrayList(ErrorInfo).init(allocator),
        };
    }

    pub fn parse(self: *Parser) !*Node {
        return self.parseExpr();
    }

    fn next(self: *Parser) !u8 {
        if (self.pos >= self.source.len) {
            self.errors.append(ErrorInfo{
                .line = self.line,
                .col = self.col,
                .msg = "unexpected end of file",
            }) catch unreachable;

            return error.ParseError;
        }

        return self.source[self.pos];
    }

    pub fn expect(self: *Parser, token: u8) !void {
        const tok = try self.next();
        if (tok != token) {
            self.errors.append(ErrorInfo{
                .line = self.line,
                .col = self.col,
                .msg = "unexpected token",
            }) catch unreachable;

            return error.ParseError;
        }

        self.pos += 1;
        self.col += 1;

        return;
    }

    pub fn skipWhitespace(self: *Parser) !void {
        while (true) {
            const tok = try self.next();
            switch (tok) {
                ' ', '\t' => {
                    self.pos += 1;
                    self.col += 1;
                },
                '\r' => {
                    self.pos += 1;
                    self.col = 1;
                },

                '\n' => {
                    self.pos += 1;
                    self.line += 1;
                    self.col = 1;
                },

                else => break,
            }

            if (self.pos >= self.source.len) {
                break;
            }

            return;
        }
    }

    fn parseIdentifier(self: *Parser) !*Node {
        const start = self.pos;
        while (try self.next() >= 'a' and try self.next() <= 'z') {
            self.pos += 1;
            self.col += 1;
        }

        const name = self.source[start..self.pos];
        var node = try self.allocator.create(Node);
        node.* = Node{
            .Identifier = .{
                .name = name,
            },
        };

        return node;
    }

    fn parseInteger(self: *Parser) !*Node {
        const start = self.pos;
        while (try self.next() >= '0' and try self.next() <= '9') {
            self.pos += 1;
            self.col += 1;
        }

        if (try self.next() == '.') {
            return self.parseFloat();
        }

        const value = try std.fmt.parseInt(i64, self.source[start..self.pos], 10);
        var node = try self.allocator.create(Node);
        node.* = Node{
            .Integer = .{
                .value = value,
            },
        };

        return node;
    }

    fn parseFloat(self: *Parser) !*Node {
        const start = self.pos;
        while (try self.next() >= '0' and try self.next() <= '9') {
            self.pos += 1;
            self.col += 1;
        }

        self.pos += 1;
        self.col += 1;

        while (try self.next() >= '0' and try self.next() <= '9') {
            self.pos += 1;
            self.col += 1;
        }

        std.log.debug("float: {s}", .{self.source[start..self.pos]});

        const value = try std.fmt.parseFloat(f64, self.source[start..self.pos]);
        var node = try self.allocator.create(Node);
        node.* = Node{
            .Float = .{
                .value = value,
            },
        };

        return node;
    }

    fn parseString(self: *Parser) !*Node {
        const start = self.pos;
        while (try self.next() != '"') {
            self.pos += 1;
            self.col += 1;
        }

        const value = self.source[start..self.pos];
        var node = try self.allocator.create(Node);
        node.* = Node{
            .String = .{
                .value = value,
            },
        };

        return node;
    }

    fn parseAttr(self: *Parser) !*Node {
        // x = y
        // x.y = z
        // x = 1

        const name = try self.parseIdentifier();
        try self.skipWhitespace();
        try self.expect('=');
        try self.skipWhitespace();
        const value = try self.parseExpr();

        var node = try self.allocator.create(Node);
        node.* = Node{
            .Attr = .{
                .name = name,
                .value = value,
            },
        };

        return node;
    }

    fn parseSet(self: *Parser) !*Node {
        // { x = y; y = z }
        // { x = 1; y = 2; z = 3 }

        var items = std.ArrayList(*Node).init(self.allocator);
        defer items.deinit();

        try self.expect('{');
        try self.skipWhitespace();

        while (try self.next() != '}') {
            const item = try self.parseAttr();
            items.append(item) catch unreachable;
            try self.skipWhitespace();

            if (try self.next() == ';') {
                self.pos += 1;
                self.col += 1;
                try self.skipWhitespace();
            }
        }

        try self.expect('}');
        try self.skipWhitespace();

        var node = try self.allocator.create(Node);
        node.* = Node{
            .Set = .{
                .items = try items.toOwnedSlice(),
            },
        };

        return node;
    }

    fn parseList(self: *Parser) !*Node {
        // [ x y z ]
        // [ 1 2 3 ]

        var items = std.ArrayList(*Node).init(self.allocator);
        defer items.deinit();

        try self.expect('[');
        try self.skipWhitespace();

        while (try self.next() != ']') {
            const item = try self.parseExpr();
            items.append(item) catch unreachable;
            try self.skipWhitespace();

            if (try self.next() == ',') {
                self.pos += 1;
                self.col += 1;
                try self.skipWhitespace();
            }
        }

        try self.expect(']');
        try self.skipWhitespace();

        var node = try self.allocator.create(Node);
        node.* = Node{
            .List = .{
                .items = try items.toOwnedSlice(),
            },
        };

        return node;
    }

    fn parseLambda(self: *Parser) !*Node {
        // x: y: x + y
        // x: x + 1

        const start = self.pos;
        while (try self.next() >= 'a' and try self.next() <= 'z') {
            self.pos += 1;
            self.col += 1;
        }

        if (try self.next() != ':') {
            return error.ParseError;
        }

        var params = std.ArrayList(*Node).init(self.allocator);
        defer params.deinit();

        while (try self.next() == ':') {
            self.pos += 1;
            self.col += 1;

            const name = self.source[start..self.pos];
            var ident = try self.allocator.create(Node);
            ident.* = Node{
                .Identifier = .{
                    .name = name,
                },
            };

            params.append(ident) catch unreachable;

            try self.skipWhitespace();
        }

        const body = try self.parseExpr();

        var node = try self.allocator.create(Node);
        node.* = Node{
            .Lambda = .{
                .params = try params.toOwnedSlice(),
                .body = body,
            },
        };

        return node;
    }

    fn parseExpr(self: *Parser) anyerror!*Node {
        const tok = try self.next();

        return self.parseLambda() catch {
            switch (tok) {
                'a'...'z' => return self.parseIdentifier(),
                '0'...'9' => return self.parseFloat(),
                '"' => return self.parseString(),
                '{' => return self.parseSet(),
                '[' => return self.parseList(),
                else => {
                    self.errors.append(ErrorInfo{
                        .line = self.line,
                        .col = self.col,
                        .msg = "unexpected token",
                    }) catch unreachable;

                    return error.ParseError;
                },
            }
        };
    }
};
