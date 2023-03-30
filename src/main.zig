const std = @import("std");
const Parser = @import("parser.zig").Parser;
const Node = @import("parser.zig").Node;

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();

    var stdin = std.io.getStdIn().reader();
    var stderr = std.io.getStdErr().writer();

    const source = try stdin.readAllAlloc(arena.allocator(), 1024);

    var parser = Parser.init(
        arena.allocator(),
        source,
    );

    const node = parser.parse() catch |err| {
        for (try parser.errors.toOwnedSlice()) |error_info| {
            try stderr.print("  :{d}:{d}: {s}\n", .{
                error_info.line,
                error_info.col,
                error_info.msg,
            });
        }
        return err;
    };

    try stderr.print("{any}", .{node});
}
