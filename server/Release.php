<?php
declare(strict_types=1);
/** Prevent stale update responses before token consumption or streaming. */
final class Release {
 public static function check(string $version, mixed $current, mixed $minimum): void {
  foreach ([$current,$minimum] as $value) {
   if($value!==null&&(!is_string($value)||strlen($value)>32||!preg_match('/^[0-9]+(?:\.[0-9]+){0,3}$/D',$value)))throw new DomainException('Ungültige Versionsangabe.',422);
  }
  if(($current!==null&&version_compare($version,$current,'<='))||($minimum!==null&&version_compare($version,$minimum,'<')))throw new DomainException('Die angeforderte neue Version ist noch nicht verfügbar. Eine ältere Datei wird nicht ausgeliefert.',409);
 }
}
