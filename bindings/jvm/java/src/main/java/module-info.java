/**
 * cleanrules for Java: a rules engine with explainable decisions, on the
 * cleanrules C ABI through the Foreign Function &amp; Memory API.
 *
 * <p>{@code io.github.cleanrules} holds what every layer shares (errors,
 * enums, limits, versions); {@code io.github.cleanrules.ffi} is the native
 * layer. The binding calls restricted methods of {@code java.lang.foreign}:
 * run the program with {@code --enable-native-access=io.github.cleanrules} (on
 * the module path) or {@code --enable-native-access=ALL-UNNAMED} (on the class
 * path). Without it Java warns the first time the library loads, and with
 * {@code --illegal-native-access=deny} loading fails with an
 * {@link java.lang.UnsatisfiedLinkError} that names the flag.
 */
module io.github.cleanrules {
    exports io.github.cleanrules;
    exports io.github.cleanrules.ffi;
}
