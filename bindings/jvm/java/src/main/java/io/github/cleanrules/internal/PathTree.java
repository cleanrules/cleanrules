package io.github.cleanrules.internal;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * The field paths a rule set or a node reads, as a tree in first-seen order:
 * the reader walks native data along it, each object once
 * ({@code spec/data-model.md}, <i>Reading native data</i>).
 */
public final class PathTree {
    /** Whether a path ends here: the value here is converted whole. */
    boolean end;
    /** The keys read from here. */
    final Map<String, PathTree> keys = new LinkedHashMap<>();
    /** The indexes read from here. */
    final Map<Integer, PathTree> indices = new LinkedHashMap<>();

    /**
     * The tree of {@code paths}: lists of String keys, Integer indexes and Long
     * indexes. A Long index is beyond any Java list or array, so the path
     * stops before it: the container there is still read (a list with its
     * length, a map without the keys this path would read), and the core finds
     * the index missing, with the reason of any other index out of a list.
     */
    public static PathTree of(List<List<Object>> paths) {
        PathTree root = new PathTree();
        for (List<Object> path : paths) {
            PathTree node = root;
            boolean whole = true;
            for (Object segment : path) {
                if (segment instanceof Integer index) {
                    node = node.indices.computeIfAbsent(index, ignored -> new PathTree());
                } else if (segment instanceof String key) {
                    node = node.keys.computeIfAbsent(key, ignored -> new PathTree());
                } else {
                    whole = false;
                    break;
                }
            }
            node.end |= whole;
        }
        return root;
    }
}
