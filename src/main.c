/*
 *    Copyright 2025 Karesis
 *
 *    Licensed under the Apache License, Version 2.0 (the "License");
 *    you may not use this file except in compliance with the License.
 *    You may obtain a copy of the License at
 *
 *        http://www.apache.org/licenses/LICENSE-2.0
 *
 *    Unless required by applicable law or agreed to in writing, software
 *    distributed under the License is distributed on an "AS IS" BASIS,
 *    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *    See the License for the specific language governing permissions and
 *    limitations under the License.
 */

#include <core/macros.h>
#include <std/fs/dir.h>
#include <std/fs/path.h>
#include <core/result.h>
#include <std/fs.h>
#include <std/allocers/system.h>
#include <std/env.h>
#include <std/strings/string.h>

/// helper macro: Check if char is a path separator (Windows/Unix compatible)
#define IS_PATH_SEP(c) ((c) == '/' || (c) == '\\')

/// version number
#define LICE_VERSION "0.1.0"

/*
 * ==========================================================================
 * Structs
 * ==========================================================================
 */

static const char *USAGE_INFO =
	"lice - Automate source code license headers\n"
	"\n"
	"Usage:\n"
	"    lice [options] [paths...]\n"
	"\n"
	"Arguments:\n"
	"    [paths]                  Directories or files to process.\n"
	"                             If omitted, the current directory is used.\n"
	"\n"
	"Options:\n"
	"    -f, --file <path>        Path to the license header file (Required).\n"
	"    -e, --exclude <pattern>  Exclude file/directory matching this pattern.\n"
	"                             Can be specified multiple times.\n"
	"    -h, --help               Show this help message.\n"
	"\n"
	"Examples:\n"
	"    # Apply license to the current directory\n"
	"    lice -f HEADER.txt\n"
	"\n"
	"    # Apply to 'src' and 'include', excluding 'vendor' and 'build'\n"
	"    lice -f HEADER.txt -e vendor -e build src include\n"
	"\n";

defVec(str_t, StrList);
defResult(bool, const char *, AppRes);

struct LiceConfig {
	str_t license_file; /// -f
	StrList excludes; /// -e (can have multiple)
	StrList targets; /// <paths ...> (can have multiple)
	allocer_t alc; /// save allocator reference for cleanup
};

struct WalkCtx {
	const struct LiceConfig *cfg;
	str_t golden_header;
};

/*
 * ==========================================================================
 * Forward Declarations
 * ==========================================================================
 */

AppRes run(allocer_t sys, int argc, char **argv);
AppRes run_logic(allocer_t sys, const struct LiceConfig *cfg);
static bool apply_license_to_file(const char *filepath, str_t golden_header);
static bool license_walk_cb(const char *path, dir_entry_type_t type,
			    void *userdata);
void cleanup_config(struct LiceConfig *cfg);
static void format_license_as_comment(string_t *out, str_t raw_license);
static bool is_path_excluded(str_t path, str_t pattern);

/*
 * ==========================================================================
 * Entry Point
 * ==========================================================================
 */

int main(int argc, char **argv)
{
	auto sys = allocer_system();
	auto res = run(sys, argc, argv);
	if (is_err(res)) {
		die("Error: %s\n%s", res.err, USAGE_INFO);
	}
	return 0;
}

AppRes run(allocer_t sys, int argc, char **argv)
{
	/// --- 1. Infrastructure Initialization ---
	defer(args_deinit) args_t args;
	verify(AppRes, args_init(&args, sys, argc, argv), "Args init failed");

	/// --- 2. Configuration Initialization (RAII) ---
	defer(cleanup_config) struct LiceConfig cfg;
	cfg.alc = sys;
	/// initialize vector
	verify(AppRes, vec_init(cfg.excludes, sys, 4), "Vec init failed");
	verify(AppRes, vec_init(cfg.targets, sys, 8), "Vec init failed");

	/// skip program name (argv[0])
	if (args_has_next(&args))
		args_next(&args);

	/// --- 3. Argument Parsing Loop ---
	args_foreach(arg, &args)
	{
		/// case A: -f / --file (Flag with Value)
		if (str_eq_cstr(arg, "-f") || str_eq_cstr(arg, "--file")) {
			/// check if there is a next argument
			if (!args_has_next(&args)) {
				return (AppRes)err(
					"-f/--file requires an argument");
			}
			/// consume the next argument as the value
			cfg.license_file = args_next(&args);
		}

		/// case B: -e / --exclude (Flag with Value, multiple allowed)
		else if (str_eq_cstr(arg, "-e") ||
			 str_eq_cstr(arg, "--exclude")) {
			if (!args_has_next(&args)) {
				return (AppRes)err(
					"-e/--exclude requires an argument");
			}
			str_t val = args_next(&args);
			verify(AppRes, vec_push(cfg.excludes, val),
			       "OOM pushing exclude");
		}

		/// case C: --help
		else if (str_eq_cstr(arg, "--help") || str_eq_cstr(arg, "-h")) {
			printf("lice v%s\n", LICE_VERSION);
			printf("%s", USAGE_INFO);
			return (AppRes)ok(true); /// early exit
		}

		/// case D: Looks like a Flag but unrecognized
		else if (str_starts_with(arg, str("-"))) {
			/// here we could use string_fmt for a detailed error, or just fail
			return (AppRes)err("Unknown option provided");
		}

		/// case E: Normal argument (Target Paths)
		else {
			verify(AppRes, vec_push(cfg.targets, arg),
			       "OOM pushing target");
		}
	}

	/// --- 4. Validation ---
	if (str_is_empty(cfg.license_file)) {
		return (AppRes)err("Missing required argument: -f/--file");
	}

	if (vec_len(cfg.targets) == 0) {
		/// if no path specified, default to current directory
		verify(AppRes, vec_push(cfg.targets, str(".")), "OOM");
	}

	/// --- 5. Dispatch to Business Logic ---
	return run_logic(sys, &cfg);
}

/*
 * ==========================================================================
 * Business Logic
 * ==========================================================================
 */

AppRes run_logic(allocer_t sys, const struct LiceConfig *cfg)
{
	/// 1. read License template file
	defer(string_deinit) string_t raw_license;
	verify(AppRes, string_init(&raw_license, sys, 0), "OOM");

	if (!file_read_to_string(cfg->license_file.ptr, &raw_license)) {
		return (AppRes)err("Failed to read license file");
	}

	/// 2. format as comment block (Golden Header)
	defer(string_deinit) string_t golden_header;
	verify(AppRes, string_init(&golden_header, sys, raw_license.len + 100),
	       "OOM");
	format_license_as_comment(&golden_header, string_as_str(&raw_license));

	/// 3. prepare walk context
	struct WalkCtx ctx = { .cfg = cfg,
			       .golden_header = string_as_str(&golden_header) };

	/// 4. walk through all target paths
	vec_foreach(target_ptr, cfg->targets)
	{
		str_t root = *target_ptr;
		/// check if root exists
		if (!file_exists(root.ptr)) {
			log_warn("Target path not found: " fstr, fmt_str(root));
			continue;
		}

		if (!dir_walk(sys, root.ptr, license_walk_cb, &ctx)) {
			/// maybe a single file? Try processing directly
			license_walk_cb(root.ptr, DIR_ENTRY_FILE, &ctx);
		}
	}

	return (AppRes)ok(true);
}

/*
 * ==========================================================================
 * Core Implementation
 * ==========================================================================
 */

static bool apply_license_to_file(const char *filepath, str_t golden_header)
{
	allocer_t sys = allocer_system();
	defer(string_deinit) string_t content;

	if (!string_init(&content, sys, 0))
		return false;
	if (!file_read_to_string(filepath, &content)) {
		log_warn("Could not read file '%s'", filepath);
		return false;
	}

	str_t content_slice = string_as_str(&content);

	/// 2. check if Header already exists
	if (str_starts_with(content_slice, golden_header)) {
		printf("  License OK: %s\n", filepath);
		return true;
	}

	/// 3. prepare new content
	defer(string_deinit) string_t new_content;
	massert(string_init(&new_content, sys, content.len + golden_header.len),
		"OOM");

	/// 4. if there is an old Header (starts with /*), we need to replace it
	if (str_starts_with(content_slice, str("/*"))) {
		printf("  Updating license: %s\n", filepath);

		/// find position of */
		const char *end_comment = strstr(content.data, "*/");
		if (!end_comment) {
			log_warn("Skipping '%s' (malformed block comment)",
				 filepath);
			return false;
		}

		/// skip */ and following whitespace
		const char *body_start = end_comment + 2;
		while (*body_start &&
		       (*body_start == ' ' || *body_start == '\n' ||
			*body_start == '\r')) {
			body_start++;
		}

		/// concat: Golden Header + Body
		asserrt(string_append(&new_content, golden_header));
		asserrt(string_append_cstr(&new_content, body_start));
	} else {
		/// no Header, prepend directly
		printf("  Adding license: %s\n", filepath);
		asserrt(string_append(&new_content, golden_header));
		asserrt(string_append(&new_content, content_slice));
	}

	/// 5. write back to file
	return file_write(filepath, string_as_str(&new_content));
}

static bool license_walk_cb(const char *path, dir_entry_type_t type,
			    void *userdata)
{
	struct WalkCtx *ctx = (struct WalkCtx *)userdata;
	str_t path_slice = str_from_cstr(path);

	/// 1. check Exclude
	/// iterate over cfg->excludes list
	vec_foreach(ex_ptr, ctx->cfg->excludes)
	{
		auto ex_path = *ex_ptr;
		if (is_path_excluded(path_slice, ex_path)) {
			/// print debug info
			log_info("  [Exclude] Skipping: %s (matches '" fstr
				 "')\n",
				 path, fmt_str(ex_path));
			return true; /// skip
		}
	}

	/// 2. only process files
	if (type != DIR_ENTRY_FILE)
		return true;

	/// 3. check extension (.c / .h)
	str_t ext = path_ext(path_slice);
	if (!str_eq_cstr(ext, "c") && !str_eq_cstr(ext, "h")) {
		return true;
	}

	/// 4. apply License
	apply_license_to_file(path, ctx->golden_header);

	return true;
}

/*
 * ==========================================================================
 * Helpers
 * ==========================================================================
 */

/// dedicated cleanup function to use with defer
void cleanup_config(struct LiceConfig *cfg)
{
	if (cfg->alc.vtable) { /// check if initialized
		vec_deinit(cfg->excludes);
		vec_deinit(cfg->targets);
	}
}

static void format_license_as_comment(string_t *out, str_t raw_license)
{
	asserrt(string_append_cstr(out, "/*\n"));

	/// iterate over each line using str_split_lines
	str_for_lines(line, raw_license)
	{
		if (line.len == 0) {
			asserrt(string_append_cstr(
				out,
				" *\n")); /// no space for empty lines
		} else {
			asserrt(string_fmt(out, " * " fstr "\n",
					   fmt_str(line)));
		}
	}

	asserrt(string_append_cstr(out, " */\n\n"));
}

/**
 * Check if path contains exclude_pattern as an independent path component
 * E.g. pattern "temp":
 * "temp"          -> true
 * "temp/file.c"   -> true
 * "src/temp/x.c"  -> true
 * "template.c"    -> false (Boundary check effective)
 * "item_post.c"   -> false
 */
static bool is_path_excluded(str_t path, str_t pattern)
{
	usize start_idx = 0;

	while (true) {
		/// search in the remaining part
		str_t remaining = str_from_parts(path.ptr + start_idx,
						 path.len - start_idx);
		usize found_idx = str_find(remaining, pattern);

		if (found_idx == (usize)-1)
			break;

		/// convert to absolute index
		usize abs_idx = start_idx + found_idx;

		/// 1. check left boundary
		bool left_ok = (abs_idx == 0) ||
			       IS_PATH_SEP(path.ptr[abs_idx - 1]);

		/// 2. check right boundary
		usize end_idx = abs_idx + pattern.len;
		bool right_ok = (end_idx == path.len) ||
				IS_PATH_SEP(path.ptr[end_idx]);

		if (left_ok && right_ok)
			return true;

		/// continue searching for next occurrence
		start_idx = abs_idx + 1;
	}
	return false;
}
