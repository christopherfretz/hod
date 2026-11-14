# The duckdb row's project include (CMAKE_PROJECT_DuckDB_INCLUDE, WP-223).
# With DISABLE_UNITY every source is its own TU, and v1.5.6's
# src/common/allocator/allocator_jemalloc.cpp uses StringUtil without
# including its header (the unity TU got it from a neighbour; clang 19
# rejects the file alike). At the end of the top directory, once every
# subdirectory is added, that one source is given the header with -include.
cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL
  set_source_files_properties("${CMAKE_SOURCE_DIR}/src/common/allocator/allocator_jemalloc.cpp"
    DIRECTORY "${CMAKE_SOURCE_DIR}/src/common/allocator"
    PROPERTIES COMPILE_OPTIONS "-include;duckdb/common/string_util.hpp"))
