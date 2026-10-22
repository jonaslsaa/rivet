import java.nio.file.Path;
import net.minecraft.util.DirectoryLock;

/** Cross-process oracle for the actual pinned Paper DirectoryLock. */
public final class DirectoryLockProbe {
    public static void main(String[] args) throws Exception {
        Path world = Path.of(args[1]);
        if (args[0].equals("locked")) {
            if (!DirectoryLock.isLocked(world)) throw new AssertionError("expected held lock");
        } else {
            try (DirectoryLock lock = DirectoryLock.create(world)) {
                if (!lock.isValid()) throw new AssertionError("invalid acquired lock");
                System.out.println("READY");
                System.out.flush();
                if (args[0].equals("hold")) System.in.read();
            }
        }
    }
}
