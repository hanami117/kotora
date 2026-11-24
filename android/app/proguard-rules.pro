# JNA (UniFFI's bindings call the Rust library through it) finds classes and fields by reflection.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn java.awt.**
# The generated bindings: structures and callbacks JNA maps by name.
-keep class io.github.hanami117.kotora.core.** { *; }
